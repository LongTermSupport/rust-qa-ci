//! Tool catalogue, command planning and the executor that runs tools and project stages against a
//! workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::config::{Config, MutantsMode};
use crate::date::Date;
use crate::defences::{DylintLint, Engine, enumerate};
use crate::diagnostics::{Diagnostic, dedupe, in_scope, parse_cargo_messages};
use crate::finding::Finding;
use crate::hit::{self, Hit};
use crate::manifest::{dylint_libraries, has_dylint_metadata};
use crate::pipeline::{Executor, Outcome, Phase, Step};
use crate::record::{Covers, Record, lint_key, same_rule};
use crate::stage::{Output, Stage};
use crate::{Error, ast_grep, builtin, summary, suppression};

/// Every tool, declared in execution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    /// `cargo fmt`.
    Fmt,
    /// Project record validation.
    Record,
    /// Suppression guard.
    Suppression,
    /// Agent summary freshness.
    Summary,
    /// Every listed identifier resolves to documentation.
    Docs,
    /// ast-grep supplementary syntactic rules.
    AstGrep,
    /// Clippy with `-D warnings`.
    Clippy,
    /// dylint type-aware rules.
    Dylint,
    /// cargo-deny.
    Deny,
    /// cargo-audit.
    Audit,
    /// cargo-machete.
    Machete,
    /// nextest plus doctests.
    Test,
    /// cargo-llvm-cov gate.
    Coverage,
    /// cargo-mutants.
    Mutants,
}

impl Tool {
    /// The catalogue in execution order.
    pub const ALL: [Self; 14] = [
        Self::Fmt,
        Self::Record,
        Self::Suppression,
        Self::Summary,
        Self::Docs,
        Self::AstGrep,
        Self::Clippy,
        Self::Dylint,
        Self::Deny,
        Self::Audit,
        Self::Machete,
        Self::Test,
        Self::Coverage,
        Self::Mutants,
    ];

    /// Name used with `-t`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Record => "record",
            Self::Suppression => "suppression",
            Self::Summary => "summary",
            Self::Docs => "docs",
            Self::AstGrep => "ast-grep",
            Self::Clippy => "clippy",
            Self::Dylint => "dylint",
            Self::Deny => "deny",
            Self::Audit => "audit",
            Self::Machete => "machete",
            Self::Test => "test",
            Self::Coverage => "coverage",
            Self::Mutants => "mutants",
        }
    }

    /// Tool for a `-t` name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// Phase the tool belongs to.
    #[must_use]
    pub fn phase(self) -> Phase {
        match self {
            Self::Fmt => Phase::Format,
            Self::Record
            | Self::Suppression
            | Self::Summary
            | Self::Docs
            | Self::AstGrep
            | Self::Clippy
            | Self::Dylint => Phase::Detectors,
            Self::Deny
            | Self::Audit
            | Self::Machete
            | Self::Test
            | Self::Coverage
            | Self::Mutants => Phase::Runners,
        }
    }

    /// Whether the configuration enables the tool.
    #[must_use]
    pub fn enabled(self, config: &Config) -> bool {
        let tools = &config.tools;
        match self {
            Self::Fmt => tools.fmt,
            Self::Record => tools.record,
            Self::Suppression => tools.suppression,
            Self::Summary => tools.summary,
            Self::Docs => tools.docs,
            Self::AstGrep => tools.ast_grep,
            Self::Clippy => tools.clippy,
            Self::Dylint => tools.dylint,
            Self::Deny => tools.deny,
            Self::Audit => tools.audit,
            Self::Machete => tools.machete,
            Self::Test => tools.test,
            Self::Coverage => tools.coverage,
            Self::Mutants => tools.mutants,
        }
    }
}

/// Every step of a default run: each enabled tool, then each project stage, in pipeline order.
#[must_use]
pub fn all_steps(config: &Config) -> Vec<Step> {
    let mut steps: Vec<Step> = Tool::ALL
        .into_iter()
        .filter(|tool| tool.enabled(config))
        .map(Step::Tool)
        .collect();
    steps.extend(
        config
            .stages
            .iter()
            .enumerate()
            .map(|(index, stage)| Step::Stage {
                index,
                phase: stage.phase,
            }),
    );
    steps.sort();
    steps
}

/// The name a step is selected and reported by.
#[must_use]
pub fn step_name(step: Step, config: &Config) -> String {
    match step {
        Step::Tool(tool) => tool.name().to_owned(),
        Step::Stage { index, .. } => config
            .stages
            .get(index)
            .map_or_else(|| format!("stage #{index}"), |stage| stage.name.clone()),
    }
}

/// Steps to run: the named tools and stages (explicit requests win over configuration), or every
/// enabled tool and every stage.
///
/// # Errors
/// Returns [`Error::Usage`] for an unknown name.
pub fn select(names: &[String], config: &Config) -> Result<Vec<Step>, Error> {
    if names.is_empty() {
        return Ok(all_steps(config));
    }
    let mut steps = names
        .iter()
        .map(|name| {
            if let Some(tool) = Tool::from_name(name) {
                return Ok(Step::Tool(tool));
            }
            if let Some((index, stage)) = config
                .stages
                .iter()
                .enumerate()
                .find(|(_, stage)| stage.name == *name)
            {
                return Ok(Step::Stage {
                    index,
                    phase: stage.phase,
                });
            }
            let mut known: Vec<&str> = Tool::ALL.iter().map(|t| t.name()).collect();
            known.extend(config.stages.iter().map(|s| s.name.as_str()));
            Err(Error::Usage(format!(
                "unknown tool `{name}`; known tools and stages: {}",
                known.join(", ")
            )))
        })
        .collect::<Result<Vec<_>, _>>()?;
    steps.sort();
    steps.dedup();
    Ok(steps)
}

/// Everything a run needs to know about the workspace.
#[derive(Debug, Clone)]
pub struct Context {
    /// Workspace root.
    pub root: PathBuf,
    /// Effective configuration.
    pub config: Config,
    /// Paths (relative to root) the run is limited to; empty means everything.
    pub scope: Vec<PathBuf>,
    /// Packages owning the scoped paths.
    pub packages: Vec<String>,
    /// Whether the selected packages have library targets (doctests apply).
    pub doctests: bool,
    /// CI mode: autofix, then fail on a dirty tree.
    pub ci: bool,
    /// Whether formatting may modify files.
    pub fix: bool,
}

impl Context {
    /// Builds a context, resolving owning packages through `cargo metadata`.
    ///
    /// # Errors
    /// Returns an error when `cargo metadata` fails.
    pub fn discover(
        root: &Path,
        config: Config,
        scope: Vec<PathBuf>,
        ci: bool,
        fix: bool,
    ) -> Result<Self, Error> {
        let output = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(root)
            .output()
            .map_err(|e| command_error("cargo metadata", &e.to_string()))?;
        if !output.status.success() {
            return Err(command_error(
                "cargo metadata",
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        let metadata: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| command_error("cargo metadata", &e.to_string()))?;
        let (packages, doctests) = if scope.is_empty() {
            let all = metadata
                .get("packages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            (Vec::new(), all.iter().any(has_lib))
        } else {
            owning_packages(&metadata, root, &scope)
        };
        Ok(Self {
            root: root.to_path_buf(),
            config,
            scope,
            packages,
            doctests,
            ci,
            fix,
        })
    }

    /// The further workspaces this run checks: all of them, or those holding a scoped path.
    #[must_use]
    pub fn workspaces(&self) -> Vec<&Path> {
        self.config
            .workspaces
            .iter()
            .map(PathBuf::as_path)
            .filter(|dir| self.scope.is_empty() || self.scope.iter().any(|p| p.starts_with(dir)))
            .collect()
    }

    fn main_in_scope(&self) -> bool {
        self.scope.is_empty() || !self.packages.is_empty()
    }
}

fn command_error(command: &str, message: &str) -> Error {
    Error::Command {
        command: command.to_owned(),
        message: message.to_owned(),
    }
}

fn has_lib(package: &Value) -> bool {
    package
        .get("targets")
        .and_then(Value::as_array)
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target
                    .get("kind")
                    .and_then(Value::as_array)
                    .is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|k| matches!(k.as_str(), Some("lib" | "rlib")))
                    })
            })
        })
}

/// Packages owning `scope` paths (the innermost package containing a path, plus every package inside
/// a scoped directory), and whether any of them has a library target.
#[must_use]
pub fn owning_packages(metadata: &Value, root: &Path, scope: &[PathBuf]) -> (Vec<String>, bool) {
    let packages: Vec<(String, PathBuf, bool)> = metadata
        .get("packages")
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|p| {
                    let name = p.get("name")?.as_str()?.to_owned();
                    let manifest = PathBuf::from(p.get("manifest_path")?.as_str()?);
                    let dir = manifest.parent()?.strip_prefix(root).ok()?.to_path_buf();
                    Some((name, dir, has_lib(p)))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut chosen: Vec<(String, bool)> = Vec::new();
    for wanted in scope {
        let innermost = packages
            .iter()
            .filter(|(_, dir, _)| wanted.starts_with(dir))
            .max_by_key(|(_, dir, _)| dir.components().count());
        chosen.extend(innermost.map(|(name, _, lib)| (name.clone(), *lib)));
        chosen.extend(
            packages
                .iter()
                .filter(|(_, dir, _)| dir.starts_with(wanted) && dir != wanted)
                .map(|(name, _, lib)| (name.clone(), *lib)),
        );
    }
    chosen.sort();
    chosen.dedup();
    let doctests = chosen.iter().any(|(_, lib)| *lib);
    (chosen.into_iter().map(|(name, _)| name).collect(), doctests)
}

/// One external command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    /// Program.
    pub program: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Working directory relative to the root; `None` is the root.
    pub dir: Option<PathBuf>,
    /// Whether the command chooses its own Rust toolchain (from its directory's
    /// `rust-toolchain.toml`) rather than inheriting the one `rust-qa` runs under.
    pub own_toolchain: bool,
    /// Variables set for the command.
    pub env: Vec<(String, String)>,
}

impl Cmd {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            dir: None,
            own_toolchain: false,
            env: Vec::new(),
        }
    }

    fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    fn args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.args.extend(args);
        self
    }

    fn locked(self, locked: bool) -> Self {
        if locked { self.arg("--locked") } else { self }
    }

    /// Runs the command in workspace `dir`, with that workspace's own toolchain.
    fn within(mut self, dir: &Path) -> Self {
        self.dir = Some(dir.to_path_buf());
        self.own_toolchain = true;
        self
    }

    /// The command line, for the operator.
    #[must_use]
    pub fn display(&self) -> String {
        let line = format!("{} {}", self.program, self.args.join(" "));
        let line = line.trim_end().to_owned();
        match &self.dir {
            Some(dir) => format!("(cd {} && {line})", dir.display()),
            None => line,
        }
    }

    /// The process to spawn, without the variables `cargo run` sets to describe the package it
    /// runs: they describe rust-qa's caller, not the workspace under check, and tools act on
    /// them (cargo-machete takes `CARGO_PKG_NAME` to mean it was not started as `cargo machete`).
    /// A command with its own toolchain also loses the variables rustup and cargo set for the
    /// toolchain `rust-qa` runs under, which would otherwise override that choice.
    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        for (name, _) in std::env::vars_os() {
            let name = name.to_string_lossy().into_owned();
            if is_package_variable(&name) || (self.own_toolchain && is_toolchain_variable(&name)) {
                command.env_remove(name);
            }
        }
        for (name, value) in &self.env {
            command.env(name, value);
        }
        command
    }
}

/// Whether `name` is one of the variables Cargo sets for a package's binaries and tests.
fn is_package_variable(name: &str) -> bool {
    name.starts_with("CARGO_PKG_")
        || name.starts_with("CARGO_BIN_EXE_")
        || matches!(
            name,
            "CARGO_MANIFEST_DIR"
                | "CARGO_MANIFEST_PATH"
                | "CARGO_CRATE_NAME"
                | "CARGO_BIN_NAME"
                | "CARGO_PRIMARY_PACKAGE"
                | "CARGO_TARGET_TMPDIR"
        )
}

/// Whether `name` is a variable that pins the toolchain or the build: rustup's, rustc's and
/// cargo's own, except `CARGO_HOME`, where the tools are installed.
fn is_toolchain_variable(name: &str) -> bool {
    name == "RUSTUP_TOOLCHAIN"
        || name.starts_with("RUSTC")
        || name.starts_with("RUSTDOC")
        || (name.starts_with("CARGO") && name != "CARGO_HOME")
}

/// Mutation diff location for `diff` mode, relative to the root.
pub const MUTANTS_DIFF: &str = "target/rust-qa/mutants.diff";

/// External commands for `tool` (empty for in-process tools and disabled modes): the workspace's
/// own, then each further workspace's.
#[must_use]
pub fn plan(tool: Tool, ctx: &Context) -> Vec<Cmd> {
    let package_bound = matches!(
        tool,
        Tool::Fmt | Tool::Clippy | Tool::Dylint | Tool::Test | Tool::Coverage
    );
    let mut cmds = if !package_bound || ctx.main_in_scope() {
        main_plan(tool, ctx)
    } else {
        Vec::new()
    };
    for dir in ctx.workspaces() {
        cmds.extend(workspace_plan(tool, ctx, dir));
    }
    cmds
}

/// Whether the workspace at `dir` has a `Cargo.lock`, so that cargo runs with `--locked` and a stale
/// lock file fails rather than being rewritten.
fn has_lock(dir: &Path) -> bool {
    dir.join("Cargo.lock").is_file()
}

fn main_plan(tool: Tool, ctx: &Context) -> Vec<Cmd> {
    let scoped = !ctx.scope.is_empty();
    let locked = has_lock(&ctx.root);
    let packages: Vec<String> = if scoped {
        ctx.packages
            .iter()
            .flat_map(|p| ["-p".to_owned(), p.clone()])
            .collect()
    } else {
        vec!["--workspace".to_owned()]
    };
    match tool {
        Tool::Fmt => {
            let cmd = Cmd::new("cargo", &["fmt"]);
            let cmd = if scoped {
                cmd.args(packages)
            } else {
                cmd.arg("--all")
            };
            vec![if ctx.fix { cmd } else { cmd.arg("--check") }]
        }
        Tool::Clippy => {
            let cmd = Cmd::new("cargo", &["clippy"])
                .args(packages)
                .arg("--all-targets")
                .arg("--all-features")
                .locked(locked);
            // JSON on every run, so that each finding prints its lint name unaltered.
            vec![
                cmd.arg("--message-format=json")
                    .arg("--")
                    .arg("-D")
                    .arg("warnings"),
            ]
        }
        Tool::Dylint => vec![
            Cmd::new("cargo", &["dylint", "--all"])
                .args(packages)
                .arg("--")
                .arg("--all-targets")
                .arg("--all-features")
                .locked(locked)
                .arg("--message-format=json"),
        ],
        Tool::AstGrep => ast_grep_plan(ctx),
        Tool::Deny => vec![Cmd::new(
            "cargo",
            &[
                "deny",
                "--format",
                "json",
                "--workspace",
                "--all-features",
                "check",
            ],
        )],
        Tool::Audit => vec![Cmd::new("cargo", &["audit"])],
        Tool::Machete => vec![Cmd::new("cargo", &["machete"])],
        Tool::Test => {
            let mut cmds = vec![
                Cmd::new("cargo", &["nextest", "run"])
                    .args(packages.clone())
                    .arg("--all-features")
                    .locked(locked),
            ];
            if ctx.doctests {
                cmds.push(
                    Cmd::new("cargo", &["test"])
                        .args(packages)
                        .arg("--all-features")
                        .arg("--doc")
                        .locked(locked),
                );
            }
            cmds
        }
        Tool::Coverage => vec![
            Cmd::new("cargo", &["llvm-cov", "nextest"])
                .args(packages)
                .arg("--all-features")
                .locked(locked)
                .arg("--fail-under-lines")
                .arg(ctx.config.coverage.fail_under_lines.to_string()),
        ],
        Tool::Mutants => match ctx.config.mutants.mode {
            MutantsMode::Off => Vec::new(),
            MutantsMode::Full => vec![Cmd::new("cargo", &["mutants", "--workspace"])],
            MutantsMode::Diff => vec![Cmd::new(
                "cargo",
                &["mutants", "--workspace", "--in-diff", MUTANTS_DIFF],
            )],
        },
        Tool::Record | Tool::Suppression | Tool::Summary | Tool::Docs => Vec::new(),
    }
}

/// Commands for a further workspace, run in its directory with its own toolchain. Coverage,
/// mutation testing and the dependency runners stay with the main workspace.
fn workspace_plan(tool: Tool, ctx: &Context, dir: &Path) -> Vec<Cmd> {
    let locked = has_lock(&ctx.root.join(dir));
    match tool {
        Tool::Fmt => {
            let cmd = Cmd::new("cargo", &["fmt", "--all"]);
            vec![if ctx.fix { cmd } else { cmd.arg("--check") }.within(dir)]
        }
        Tool::Clippy => {
            let cmd = Cmd::new(
                "cargo",
                &["clippy", "--workspace", "--all-targets", "--all-features"],
            )
            .locked(locked);
            vec![
                cmd.arg("--message-format=json")
                    .arg("--")
                    .arg("-D")
                    .arg("warnings")
                    .within(dir),
            ]
        }
        Tool::Dylint => dylint_libraries(&ctx.root)
            .unwrap_or_default()
            .iter()
            .map(|library| {
                Cmd::new("cargo", &["dylint"])
                    .args(library_flags(&ctx.root, library))
                    .arg("--workspace")
                    .arg("--")
                    .arg("--all-targets")
                    .arg("--all-features")
                    .locked(locked)
                    .arg("--message-format=json")
                    .within(dir)
            })
            .collect(),
        Tool::Test => vec![
            Cmd::new("cargo", &["test", "--workspace", "--all-features"])
                .locked(locked)
                .within(dir),
        ],
        _ => Vec::new(),
    }
}

/// `cargo dylint` options loading one `[workspace.metadata.dylint]` library entry from another
/// directory: a `path` made absolute, or a `git` source with its revision and pattern.
#[must_use]
pub fn library_flags(root: &Path, library: &toml::Table) -> Vec<String> {
    let text = |key: &str| library.get(key).and_then(toml::Value::as_str);
    let mut flags = Vec::new();
    if let Some(path) = text("path") {
        flags.push("--path".to_owned());
        flags.push(root.join(path).display().to_string());
    }
    if let Some(url) = text("git") {
        flags.push("--git".to_owned());
        flags.push(url.to_owned());
        for key in ["rev", "tag", "branch"] {
            if let Some(value) = text(key) {
                flags.push(format!("--{key}"));
                flags.push(value.to_owned());
            }
        }
    }
    if let Some(pattern) = text("pattern") {
        flags.push("--pattern".to_owned());
        flags.push(pattern.to_owned());
    }
    flags
}

/// The project's own rules (when `sgconfig.yml` exists), then the bundled pack (unless switched
/// off; excluding `scan_exclude`, since the project's ast-grep ignores do not apply to it), each
/// printing JSON for the record to be applied; then the project rules' own tests, when
/// `sgconfig.yml` declares `testConfigs`.
fn ast_grep_plan(ctx: &Context) -> Vec<Cmd> {
    let scope = || ctx.scope.iter().map(|p| p.display().to_string());
    let mut cmds = Vec::new();
    let project = ctx.root.join("sgconfig.yml").is_file();
    if project {
        cmds.push(Cmd::new("ast-grep", &["scan", "--json=compact"]).args(scope()));
    }
    if ctx.config.ast_grep.bundled {
        let excludes = ctx
            .config
            .scan_exclude
            .iter()
            .flat_map(|glob| ["--globs".to_owned(), format!("!{glob}")]);
        cmds.push(
            Cmd::new(
                "ast-grep",
                &["scan", "--json=compact", "--rule", ast_grep::PACK_PATH],
            )
            .args(excludes)
            .args(scope()),
        );
    }
    if project && ast_grep::has_tests(&ctx.root).unwrap_or(false) {
        cmds.push(Cmd::new("ast-grep", &["test", "--skip-snapshot-tests"]));
    }
    cmds
}

/// Lists the lints of the configured dylint libraries; normally
/// [`crate::defences::dylint_lints`].
pub type DylintLister = fn(&Path) -> Result<Vec<DylintLint>, Error>;

/// Runs tools and stages for real against the workspace in `ctx`.
pub struct RealExecutor {
    /// Workspace context.
    pub ctx: Context,
    /// Lists dylint lints (for the summary and the dylint detector).
    pub dylint_lister: DylintLister,
}

impl Executor for RealExecutor {
    fn run(&mut self, step: Step) -> Outcome {
        println!(
            "==> [{}] {}",
            step.phase().name(),
            step_name(step, &self.ctx.config)
        );
        let result = match step {
            Step::Tool(tool) => self.run_tool(tool),
            Step::Stage { index, .. } => match self.ctx.config.stages.get(index) {
                Some(stage) => self.stage(stage),
                None => Err(Error::Usage(format!("no stage #{index}"))),
            },
        };
        match result {
            Ok(outcome) => {
                if let Outcome::Skipped(reason) = &outcome {
                    println!("    skipped: {reason}");
                }
                outcome
            }
            Err(error) => {
                println!("error: {error}");
                Outcome::Failed
            }
        }
    }
}

impl RealExecutor {
    fn run_tool(&self, tool: Tool) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let root = &ctx.root;
        let scoped = !ctx.scope.is_empty();
        let package_bound = matches!(
            tool,
            Tool::Fmt | Tool::Clippy | Tool::Dylint | Tool::Test | Tool::Coverage
        );
        if scoped && package_bound && plan(tool, ctx).is_empty() {
            return Ok(Outcome::Skipped(
                "no package owns the scoped paths".to_owned(),
            ));
        }
        match tool {
            Tool::Record => self.record(),
            Tool::Suppression => self.suppression(),
            Tool::Summary => self.summary(),
            Tool::Docs => self.docs(),
            Tool::AstGrep => self.ast_grep(),
            Tool::Dylint if !has_dylint_metadata(root)? => Ok(Outcome::Skipped(
                "no [workspace.metadata.dylint] libraries".to_owned(),
            )),
            Tool::Dylint => self.dylint(),
            Tool::Deny if !root.join("deny.toml").is_file() => {
                Ok(Outcome::Skipped("no deny.toml".to_owned()))
            }
            Tool::Mutants if ctx.config.mutants.mode == MutantsMode::Off => {
                Ok(Outcome::Skipped("mutants mode is off".to_owned()))
            }
            Tool::Mutants if ctx.config.mutants.mode == MutantsMode::Diff => self.mutants_diff(),
            Tool::Clippy => self.compiler_diagnostics(tool),
            Tool::Deny => self.deny(),
            Tool::Fmt => {
                let outcome = self.commands(tool)?;
                if outcome == Outcome::Passed && ctx.ci && ctx.fix {
                    return self.clean_tree();
                }
                Ok(outcome)
            }
            _ => self.commands(tool),
        }
    }

    fn report(findings: &[Finding]) -> Outcome {
        for finding in findings {
            println!("{finding}");
        }
        if findings.is_empty() {
            Outcome::Passed
        } else {
            Outcome::Failed
        }
    }

    fn load_record(&self) -> Result<Record, Error> {
        Record::load(&self.ctx.root.join(&self.ctx.config.record.path))
    }

    fn record(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let policy = &ctx.config.record;
        let mut findings =
            match crate::policy::gate(&ctx.root, policy, Date::today(), ctx.fix && !ctx.ci) {
                Ok(report) => {
                    for line in report.status.to_string().lines() {
                        println!("    {line}");
                    }
                    for note in &report.notes {
                        println!("    note: {note}");
                    }
                    report.findings
                }
                Err(error) => vec![Finding {
                    rule: builtin::RECORD_INVALID,
                    path: policy.path.clone(),
                    line: 0,
                    message: error.to_string(),
                }],
            };
        let record = self.load_record()?;
        if record
            .exceptions
            .iter()
            .any(|e| e.covers == Covers::Findings)
        {
            let defences = enumerate(&ctx.root, &ctx.config, &self.dylint_lister)?;
            findings.extend(findings_exceptions_unhosted(
                &record,
                &defences,
                &policy.path,
            ));
        }
        Ok(Self::report(&findings))
    }

    fn suppression(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let record_path = &ctx.config.record.path;
        let record = self.load_record()?;
        let exclude = crate::glob_set(&ctx.config.scan_exclude)?;
        let result = suppression::scan(&ctx.root, &ctx.scope, &exclude)?;
        let findings = suppression::check(&result, &record, record_path, ctx.scope.is_empty());
        println!("    {} suppression(s) found", result.suppressions.len());
        Ok(Self::report(&findings))
    }

    fn summary(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let Some(file) = &ctx.config.summary.file else {
            return Ok(Outcome::Skipped("no [summary] file configured".to_owned()));
        };
        let fresh = render_summary(&ctx.root, &ctx.config, self.dylint_lister)?;
        let path = ctx.root.join(file);
        let existing = if path.exists() {
            crate::read_to_string(&path)?
        } else {
            String::new()
        };
        let updated = summary::splice(&existing, &fresh);
        if updated == existing {
            return Ok(Outcome::Passed);
        }
        if ctx.fix && !ctx.ci {
            std::fs::write(&path, updated).map_err(|source| Error::Write {
                path: path.clone(),
                source,
            })?;
            println!("    updated {}", file.display());
            return Ok(Outcome::Passed);
        }
        Ok(Self::report(&[Finding {
            rule: builtin::SUMMARY_STALE,
            path: file.clone(),
            line: 0,
            message: "generated defence summary is out of date; run `rust-qa summary`".to_owned(),
        }]))
    }

    fn docs(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let (ids, missing) = crate::docs::dangling(&ctx.root, &ctx.config, &self.dylint_lister)?;
        println!("    {} identifier(s) checked", ids.len());
        let mut findings: Vec<Finding> = missing
            .into_iter()
            .map(|id| Finding {
                rule: builtin::DOCS_DANGLING,
                path: ctx.config.docs_dir.join(crate::docs::doc_file_name(&id)),
                line: 0,
                message: format!("`{id}` has no documentation"),
            })
            .collect();
        findings.extend(
            crate::docs::orphans(&ctx.root, &ctx.config.docs_dir, &ids)?
                .into_iter()
                .map(|page| Finding {
                    rule: builtin::DOCS_ORPHAN,
                    path: page,
                    line: 0,
                    message: "documents no identifier the gate lists; delete the page or restore its defence".to_owned(),
                }),
        );
        Ok(Self::report(&findings))
    }

    fn commands(&self, tool: Tool) -> Result<Outcome, Error> {
        for cmd in plan(tool, &self.ctx) {
            if !self.status(&cmd)? {
                return Ok(Outcome::Failed);
            }
        }
        Ok(Outcome::Passed)
    }

    fn spawn(&self, cmd: &Cmd) -> Command {
        let mut command = cmd.command();
        command.current_dir(match &cmd.dir {
            Some(dir) => self.ctx.root.join(dir),
            None => self.ctx.root.clone(),
        });
        command
    }

    fn status(&self, cmd: &Cmd) -> Result<bool, Error> {
        println!("$ {}", cmd.display());
        let status = self
            .spawn(cmd)
            .status()
            .map_err(|e| missing_tool(cmd, &e))?;
        Ok(status.success())
    }

    fn output(&self, cmd: &Cmd) -> Result<std::process::Output, Error> {
        println!("$ {}", cmd.display());
        self.spawn(cmd)
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| missing_tool(cmd, &e))
    }

    /// Prints the findings the record leaves, and on a full run the findings exceptions for
    /// `hosted` rules that covered nothing; returns whether the gate passes.
    fn conclude(&self, hits: Vec<Hit>, hosted: &[String]) -> Result<bool, Error> {
        let record = self.load_record()?;
        let applied = hit::apply(hits, &record);
        for finding in &applied.kept {
            println!("{finding}");
        }
        if applied.excepted > 0 {
            println!(
                "    {} finding(s) covered by exceptions in {}",
                applied.excepted,
                self.ctx.config.record.path.display()
            );
        }
        let mut passed = applied.blocking() == 0;
        if self.ctx.scope.is_empty() {
            let stale = hit::unused(&record, &applied.used, hosted, &self.ctx.config.record.path);
            passed &= stale.is_empty();
            for finding in &stale {
                println!("{finding}");
            }
        }
        Ok(passed)
    }

    /// Runs every planned scan, so project and bundled findings are reported together, applies
    /// the record, checks that every project rule is tested, and runs the project rules' tests.
    fn ast_grep(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let cmds = plan(Tool::AstGrep, ctx);
        if cmds.is_empty() {
            return Ok(Outcome::Skipped(
                "no sgconfig.yml and the bundled pack is off".to_owned(),
            ));
        }
        if ctx.config.ast_grep.bundled {
            ast_grep::write_pack(&ctx.root)?;
        }
        let mut passed = true;
        let mut hits = Vec::new();
        for cmd in &cmds {
            if cmd.args.first().is_some_and(|a| a == "test") {
                passed &= self.status(cmd)?;
                continue;
            }
            let output = self.output(cmd)?;
            if !matches!(output.status.code(), Some(0 | 1)) {
                println!(
                    "error: `{}` did not complete ({})",
                    cmd.display(),
                    output.status
                );
                passed = false;
                continue;
            }
            match ast_grep::parse_matches(&String::from_utf8_lossy(&output.stdout)) {
                Ok(found) => hits.extend(found),
                Err(message) => {
                    println!("error: `{}` printed no findings: {message}", cmd.display());
                    passed = false;
                }
            }
        }
        let mut hosted: Vec<String> = ast_grep::bundled_ids().map(ToOwned::to_owned).collect();
        if ctx.root.join("sgconfig.yml").is_file() {
            let unnamed: Vec<Finding> = crate::defences::unnamed_ast_grep_rules(&ctx.root)?
                .into_iter()
                .map(|file| Finding {
                    rule: builtin::RULES_UNNAMED,
                    path: file,
                    line: 0,
                    message: "a rule here has no `id`, so ast-grep reports it under the file's name, which nothing lists, documents or tests".to_owned(),
                })
                .collect();
            passed &= Self::report(&unnamed) == Outcome::Passed;
            let rules = crate::defences::ast_grep_rules(&ctx.root)?;
            let tested = ast_grep::tested_rules(&ctx.root)?;
            let untested: Vec<Finding> = rules
                .iter()
                .filter(|rule| rule.severity == "error" && !tested.contains(&rule.id))
                .map(|rule| Finding {
                    rule: builtin::RULES_UNTESTED,
                    path: rule.file.clone(),
                    line: 0,
                    message: format!(
                        "`{}` has no test case with both `valid` and `invalid` code in a testConfigs directory of sgconfig.yml",
                        rule.id
                    ),
                })
                .collect();
            passed &= Self::report(&untested) == Outcome::Passed;
            hosted.extend(rules.into_iter().map(|rule| rule.id));
        }
        passed &= self.conclude(hits, &hosted)?;
        Ok(if passed {
            Outcome::Passed
        } else {
            Outcome::Failed
        })
    }

    /// Runs the dylint libraries over every workspace and fails on each finding of one of their
    /// lints the record does not cover, at whatever level it was reported, and on every compile
    /// error. With `force_warn`, each lint is forced to warn so that no level in the code can
    /// silence it.
    fn dylint(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let lints: Vec<String> = (self.dylint_lister)(&ctx.root)?
            .into_iter()
            .map(|lint| lint.name)
            .collect();
        let forced: Vec<String> = lints.iter().map(|l| format!("--force-warn={l}")).collect();
        let mut hits = Vec::new();
        let mut errors = 0usize;
        let mut build_failed = false;
        for mut cmd in plan(Tool::Dylint, ctx) {
            if ctx.config.dylint.force_warn {
                cmd.env
                    .push(("DYLINT_RUSTFLAGS".to_owned(), forced.join(" ")));
            }
            let output = self.output(&cmd)?;
            build_failed |= !output.status.success();
            let prefix = cmd.dir.clone().unwrap_or_default();
            for diagnostic in dedupe(parse_cargo_messages(&String::from_utf8_lossy(
                &output.stdout,
            ))) {
                let diagnostic = diagnostic.under(&ctx.root, &prefix);
                if !ctx.scope.is_empty() && !in_scope(&diagnostic, &ctx.root, &ctx.scope) {
                    continue;
                }
                match dylint_disposition(&diagnostic, &lints) {
                    Disposition::Lint(code) => hits.push(lint_hit(&diagnostic, &code)),
                    Disposition::Error => {
                        print!("{}", diagnostic_text(&diagnostic));
                        errors += 1;
                    }
                    Disposition::Nameless => {
                        print!("{}", diagnostic_text(&diagnostic));
                        println!(
                            "  = gate: blocking; a lint library reported this through no declared lint\n  = doc: rust-qa rule-doc {}",
                            builtin::DYLINT_NAMELESS
                        );
                        errors += 1;
                    }
                    Disposition::Other => {}
                }
            }
        }
        let mut passed = self.conclude(hits, &lints)? && errors == 0;
        if passed && build_failed {
            println!(
                "error: the build failed, so code after the failure was not checked; a recorded finding at deny level still stops the build: set [dylint] force_warn = true"
            );
            passed = false;
        }
        Ok(if passed {
            Outcome::Passed
        } else {
            Outcome::Failed
        })
    }

    /// Prints each compiler finding with its identifier, narrowed to the scoped paths on a subset
    /// run, and fails on every error and on a build that failed.
    fn compiler_diagnostics(&self, tool: Tool) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let scoped = !ctx.scope.is_empty();
        let mut blocking = 0usize;
        let mut build_failed = false;
        for cmd in plan(tool, ctx) {
            let output = self.output(&cmd)?;
            build_failed |= !output.status.success();
            let prefix = cmd.dir.clone().unwrap_or_default();
            for diagnostic in dedupe(parse_cargo_messages(&String::from_utf8_lossy(
                &output.stdout,
            ))) {
                let diagnostic = diagnostic.under(&ctx.root, &prefix);
                if scoped && !in_scope(&diagnostic, &ctx.root, &ctx.scope) {
                    continue;
                }
                print!("{}", diagnostic_text(&diagnostic));
                if diagnostic.is_error() {
                    blocking += 1;
                }
            }
        }
        if blocking > 0 {
            return Ok(Outcome::Failed);
        }
        if build_failed {
            println!(
                "{}",
                if scoped {
                    "error: the build failed outside the scoped paths; scoped results are incomplete"
                } else {
                    "error: the build failed; cargo's own message is above"
                }
            );
            return Ok(Outcome::Failed);
        }
        Ok(Outcome::Passed)
    }

    /// Runs cargo-deny and prints each of its findings under an identifier that resolves.
    fn deny(&self) -> Result<Outcome, Error> {
        let mut passed = true;
        for cmd in plan(Tool::Deny, &self.ctx) {
            println!("$ {}", cmd.display());
            let output = self
                .spawn(&cmd)
                .stdout(Stdio::inherit())
                .output()
                .map_err(|e| missing_tool(&cmd, &e))?;
            for line in deny_report(&String::from_utf8_lossy(&output.stderr)) {
                println!("{line}");
            }
            passed &= output.status.success();
        }
        Ok(if passed {
            Outcome::Passed
        } else {
            Outcome::Failed
        })
    }

    /// Runs a project stage: by its exit code, or by the findings it prints, with the record
    /// applied.
    fn stage(&self, stage: &Stage) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let scoped = !ctx.scope.is_empty();
        let files = if stage.takes_files(scoped) {
            stage.matching_files(&ctx.root, &ctx.scope)?
        } else {
            Vec::new()
        };
        let Some(argv) = stage.argv(scoped, &files) else {
            return Ok(Outcome::Skipped("no files for this stage".to_owned()));
        };
        let Some((program, rest)) = argv.split_first() else {
            return Err(Error::Usage(format!(
                "stage `{}` has an empty command",
                stage.name
            )));
        };
        let cmd = Cmd {
            program: program.clone(),
            args: rest.to_vec(),
            dir: stage.dir.clone(),
            own_toolchain: stage.dir.is_some(),
            env: Vec::new(),
        };
        if stage.output == Output::Status {
            return Ok(if self.status(&cmd)? {
                Outcome::Passed
            } else {
                Outcome::Failed
            });
        }
        let output = self.output(&cmd)?;
        if !matches!(output.status.code(), Some(0 | 1)) {
            println!(
                "error: stage `{}` did not complete ({})",
                stage.name, output.status
            );
            return Ok(Outcome::Failed);
        }
        let mut hits = crate::stage::parse_findings(
            &String::from_utf8_lossy(&output.stdout),
            stage.dir.as_deref(),
        )
        .map_err(|message| {
            command_error(&cmd.display(), &format!("printed no findings: {message}"))
        })?;
        for found in &mut hits {
            if let Ok(relative) = found.path.strip_prefix(&ctx.root) {
                found.path = relative.to_path_buf();
            }
        }
        hits.retain(|found| scoped_in(&found.path, &ctx.scope));
        let hosted: Vec<String> = match &stage.rules {
            Some(_) => crate::defences::stage_rules(&ctx.root, stage)?
                .into_iter()
                .map(|rule| rule.id)
                .collect(),
            None => Vec::new(),
        };
        Ok(if self.conclude(hits, &hosted)? {
            Outcome::Passed
        } else {
            Outcome::Failed
        })
    }

    fn clean_tree(&self) -> Result<Outcome, Error> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.ctx.root)
            .output()
            .map_err(|e| command_error("git status --porcelain", &e.to_string()))?;
        let dirty = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if dirty.is_empty() {
            return Ok(Outcome::Passed);
        }
        println!(
            "error: CI mode: the working tree changed after autofix; run `rust-qa run` locally and commit:"
        );
        println!("{dirty}");
        Ok(Outcome::Failed)
    }

    fn mutants_diff(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let git = |args: &[&str]| -> Result<String, Error> {
            let output = Command::new("git")
                .args(args)
                .current_dir(&ctx.root)
                .output()
                .map_err(|e| command_error(&format!("git {}", args.join(" ")), &e.to_string()))?;
            if !output.status.success() {
                return Err(command_error(
                    &format!("git {}", args.join(" ")),
                    String::from_utf8_lossy(&output.stderr).trim(),
                ));
            }
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        };
        let base = git(&["merge-base", &ctx.config.mutants.base, "HEAD"])?;
        let diff = git(&["diff", base.trim(), "--", "*.rs"])?;
        if diff.trim().is_empty() {
            return Ok(Outcome::Skipped(format!(
                "no Rust changes relative to {}",
                ctx.config.mutants.base
            )));
        }
        let path = ctx.root.join(MUTANTS_DIFF);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&path, diff).map_err(|source| Error::Write { path, source })?;
        self.commands(Tool::Mutants)
    }
}

fn scoped_in(path: &Path, scope: &[PathBuf]) -> bool {
    scope.is_empty() || scope.iter().any(|wanted| path.starts_with(wanted))
}

/// A compiler finding as the gate prints it: the compiler's rendering, then the identifier to
/// resolve, unaltered, when the finding has one.
#[must_use]
pub fn diagnostic_text(diagnostic: &Diagnostic) -> String {
    let mut text = diagnostic.rendered.trim_end().to_owned();
    text.push('\n');
    if let Some(code) = &diagnostic.code {
        text.push_str("  = doc: rust-qa rule-doc ");
        text.push_str(code);
        text.push('\n');
    }
    text
}

/// What the dylint lane does with one diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    /// A finding of a configured library's lint, under that name.
    Lint(String),
    /// A compiler error: printed, and it fails the gate.
    Error,
    /// A finding at a source location that carries no lint name: a library reported it outside
    /// any declared lint, so nothing could document, record or level it. It fails the gate.
    Nameless,
    /// Another tool's lint, or cargo's count of what it printed; the lanes that own them report them.
    Other,
}

/// How the dylint lane treats `diagnostic`, given the configured libraries' `lints`.
#[must_use]
pub fn dylint_disposition(diagnostic: &Diagnostic, lints: &[String]) -> Disposition {
    match diagnostic.code.as_deref() {
        Some(code) if lints.iter().any(|l| lint_key(l) == lint_key(code)) => {
            Disposition::Lint(code.to_owned())
        }
        _ if diagnostic.is_error() => Disposition::Error,
        None if !diagnostic.files.is_empty() => Disposition::Nameless,
        _ => Disposition::Other,
    }
}

/// cargo-deny's `--format json` stream as the gate prints it: each finding under its code, as
/// `cargo-deny::<code>` so that `rust-qa rule-doc` resolves it, with the crates it names. A line
/// that is not a cargo-deny record is printed as it came.
#[must_use]
pub fn deny_report(stream: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in stream.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(record) = serde_json::from_str::<Value>(raw) else {
            lines.push(raw.to_owned());
            continue;
        };
        let fields = record.get("fields").cloned().unwrap_or(Value::Null);
        let text = |key: &str| fields.get(key).and_then(Value::as_str).unwrap_or("");
        match record.get("type").and_then(Value::as_str) {
            Some("diagnostic") => {
                let code = text("code");
                lines.push(format!(
                    "{}[{}{code}]: {}",
                    text("severity"),
                    crate::docs::CARGO_DENY_PREFIX,
                    text("message")
                ));
                for label in fields
                    .get("labels")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let at = |key: &str| label.get(key).and_then(Value::as_u64).unwrap_or(0);
                    let said = label.get("message").and_then(Value::as_str).unwrap_or("");
                    lines.push(format!(
                        "  = at line {}, column {}: {}{}",
                        at("line"),
                        at("column"),
                        label.get("span").and_then(Value::as_str).unwrap_or(""),
                        if said.is_empty() {
                            String::new()
                        } else {
                            format!(" ({said})")
                        }
                    ));
                }
                for graph in fields
                    .get("graphs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(krate) = graph.get("Krate") {
                        let part = |key: &str| krate.get(key).and_then(Value::as_str).unwrap_or("");
                        lines.push(format!("  = crate: {} {}", part("name"), part("version")));
                    }
                }
                for note in fields
                    .get("notes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    lines.push(format!("  = note: {}", note.as_str().unwrap_or_default()));
                }
                if !code.is_empty() {
                    lines.push(format!(
                        "  = doc: rust-qa rule-doc {}{code}",
                        crate::docs::CARGO_DENY_PREFIX
                    ));
                }
            }
            Some("summary") => {
                for (check, counts) in fields.as_object().into_iter().flatten() {
                    let count = |key: &str| counts.get(key).and_then(Value::as_u64).unwrap_or(0);
                    lines.push(format!(
                        "    {check}: {} error(s), {} warning(s)",
                        count("errors"),
                        count("warnings")
                    ));
                }
            }
            _ => {
                let message = fields.get("message").and_then(Value::as_str);
                lines.push(message.map_or_else(|| raw.to_owned(), ToOwned::to_owned));
            }
        }
    }
    lines
}

fn lint_hit(diagnostic: &Diagnostic, code: &str) -> Hit {
    let mut rendered = diagnostic.rendered.trim_end().to_owned();
    if !diagnostic.is_error() {
        rendered.push_str(
            "\n  = gate: blocking; a dylint finding fails the gate unless the project record covers it",
        );
    }
    Hit {
        rule: code.to_owned(),
        path: diagnostic.files.first().cloned().unwrap_or_default(),
        line: diagnostic.line,
        column: diagnostic.column,
        message: diagnostic.message.clone(),
        note: None,
        rendered: Some(rendered),
        blocking: true,
    }
}

/// `rqaci.record.invalid` for each `covers = "findings"` exception whose rule no detector that
/// applies the record reports: findings exceptions apply to dylint lints, ast-grep rules and the
/// rules of project stages, never to rustc or Clippy, whose route is a recorded `expect`.
#[must_use]
pub fn findings_exceptions_unhosted(
    record: &Record,
    defences: &[crate::defences::Defence],
    record_path: &Path,
) -> Vec<Finding> {
    record
        .exceptions
        .iter()
        .enumerate()
        .filter(|(_, exception)| exception.covers == Covers::Findings)
        .filter(|(_, exception)| {
            !defences.iter().any(|defence| {
                matches!(
                    defence.engine,
                    Engine::Dylint | Engine::AstGrep | Engine::Stage
                ) && same_rule(&defence.id, &exception.rule)
            })
        })
        .map(|(index, exception)| Finding {
            rule: builtin::RECORD_INVALID,
            path: record_path.to_path_buf(),
            line: 0,
            message: format!(
                "exception #{} covers findings of `{}`, which no dylint lint, ast-grep rule or project stage reports; an exception for rustc or Clippy is a recorded `expect` in the source",
                index + 1,
                exception.rule
            ),
        })
        .collect()
}

fn missing_tool(cmd: &Cmd, error: &std::io::Error) -> Error {
    let hint = if error.kind() == std::io::ErrorKind::NotFound {
        "; install the pinned tools with rust-qa-ci's scripts/install-tools.sh"
    } else {
        ""
    };
    command_error(&cmd.display(), &format!("{error}{hint}"))
}

/// Renders the summary region for the workspace.
///
/// # Errors
/// Returns an error when defences or the record cannot be read.
pub fn render_summary(
    root: &Path,
    config: &Config,
    dylint_lister: DylintLister,
) -> Result<String, Error> {
    let mut defences = enumerate(root, config, &dylint_lister)?;
    let resolver = crate::docs::Resolver::new(root, &config.docs_dir);
    for defence in defences.iter_mut().filter(|d| d.summarised) {
        if let Some(page) = resolver.project_page(&defence.id)? {
            defence.fix = crate::docs::section_paragraph(&page, &config.summary.fix_headings)
                .unwrap_or_default();
        }
    }
    let record = Record::load(&root.join(&config.record.path))?;
    Ok(summary::render(&defences, &record, &config.record))
}
