//! Tool catalogue, command planning and the executor that runs tools against a workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::config::{Config, MutantsMode};
use crate::date::Date;
use crate::defences::{DylintLint, enumerate};
use crate::diagnostics::{dedupe, in_scope, parse_cargo_messages};
use crate::finding::Finding;
use crate::manifest::has_dylint_metadata;
use crate::pipeline::{Executor, Outcome, Phase};
use crate::record::Record;
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

/// Tools to run: the named ones (explicit requests win over configuration), or every enabled tool.
///
/// # Errors
/// Returns [`Error::Usage`] for an unknown name.
pub fn select(names: &[String], config: &Config) -> Result<Vec<Tool>, Error> {
    let mut tools = if names.is_empty() {
        Tool::ALL
            .into_iter()
            .filter(|tool| tool.enabled(config))
            .collect()
    } else {
        names
            .iter()
            .map(|name| {
                Tool::from_name(name).ok_or_else(|| {
                    let known: Vec<&str> = Tool::ALL.iter().map(|t| t.name()).collect();
                    Error::Usage(format!(
                        "unknown tool `{name}`; known tools: {}",
                        known.join(", ")
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    tools.sort();
    tools.dedup();
    Ok(tools)
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
}

impl Cmd {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
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

    fn display(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }
}

/// Mutation diff location for `diff` mode, relative to the root.
pub const MUTANTS_DIFF: &str = "target/rust-qa/mutants.diff";

/// External commands for `tool` (empty for in-process tools and disabled modes).
#[must_use]
pub fn plan(tool: Tool, ctx: &Context) -> Vec<Cmd> {
    let scoped = !ctx.scope.is_empty();
    let packages: Vec<String> = if scoped {
        ctx.packages
            .iter()
            .flat_map(|p| ["-p".to_owned(), p.clone()])
            .collect()
    } else {
        vec!["--workspace".to_owned()]
    };
    let json = |cmd: Cmd| {
        if scoped {
            cmd.arg("--message-format=json")
        } else {
            cmd
        }
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
                .arg("--all-features");
            vec![json(cmd).arg("--").arg("-D").arg("warnings")]
        }
        Tool::Dylint => {
            let cmd = Cmd::new("cargo", &["dylint", "--all"])
                .args(packages)
                .arg("--")
                .arg("--all-targets")
                .arg("--all-features");
            vec![json(cmd)]
        }
        Tool::AstGrep => ast_grep_plan(ctx),
        Tool::Deny => vec![Cmd::new(
            "cargo",
            &["deny", "--workspace", "--all-features", "check"],
        )],
        Tool::Audit => vec![Cmd::new("cargo", &["audit"])],
        Tool::Machete => vec![Cmd::new("cargo", &["machete"])],
        Tool::Test => {
            let mut cmds = vec![
                Cmd::new("cargo", &["nextest", "run"])
                    .args(packages.clone())
                    .arg("--all-features"),
            ];
            if ctx.doctests {
                cmds.push(
                    Cmd::new("cargo", &["test"])
                        .args(packages)
                        .arg("--all-features")
                        .arg("--doc"),
                );
            }
            cmds
        }
        Tool::Coverage => vec![
            Cmd::new("cargo", &["llvm-cov", "nextest"])
                .args(packages)
                .arg("--all-features")
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

/// The project's own rules (when `sgconfig.yml` exists), then the bundled pack (unless switched
/// off; excluding `scan_exclude`, since the project's ast-grep ignores do not apply to it).
fn ast_grep_plan(ctx: &Context) -> Vec<Cmd> {
    let scope = || ctx.scope.iter().map(|p| p.display().to_string());
    let mut cmds = Vec::new();
    if ctx.root.join("sgconfig.yml").is_file() {
        cmds.push(Cmd::new("ast-grep", &["scan"]).args(scope()));
    }
    if ctx.config.ast_grep.bundled {
        let excludes = ctx
            .config
            .scan_exclude
            .iter()
            .flat_map(|glob| ["--globs".to_owned(), format!("!{glob}")]);
        cmds.push(
            Cmd::new("ast-grep", &["scan", "--rule", ast_grep::PACK_PATH])
                .args(excludes)
                .args(scope()),
        );
    }
    cmds
}

/// Runs tools for real against the workspace in `ctx`.
pub struct RealExecutor {
    /// Workspace context.
    pub ctx: Context,
    /// Lists dylint lints (for the summary); normally [`crate::defences::dylint_lints`].
    pub dylint_lister: fn(&Path) -> Result<Vec<DylintLint>, Error>,
}

impl Executor for RealExecutor {
    fn run(&mut self, tool: Tool) -> Outcome {
        println!("==> [{}] {}", tool.phase().name(), tool.name());
        match self.run_tool(tool) {
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
        if scoped && package_bound && ctx.packages.is_empty() {
            return Ok(Outcome::Skipped(
                "no package owns the scoped paths".to_owned(),
            ));
        }
        match tool {
            Tool::Record => Ok(self.record()),
            Tool::Suppression => self.suppression(),
            Tool::Summary => self.summary(),
            Tool::Docs => self.docs(),
            Tool::AstGrep => self.ast_grep(),
            Tool::Dylint if !has_dylint_metadata(root)? => Ok(Outcome::Skipped(
                "no [workspace.metadata.dylint] libraries".to_owned(),
            )),
            Tool::Deny if !root.join("deny.toml").is_file() => {
                Ok(Outcome::Skipped("no deny.toml".to_owned()))
            }
            Tool::Mutants if ctx.config.mutants.mode == MutantsMode::Off => {
                Ok(Outcome::Skipped("mutants mode is off".to_owned()))
            }
            Tool::Mutants if ctx.config.mutants.mode == MutantsMode::Diff => self.mutants_diff(),
            Tool::Clippy | Tool::Dylint if scoped => self.scoped_diagnostics(tool),
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

    fn record(&self) -> Outcome {
        let ctx = &self.ctx;
        let policy = &ctx.config.record;
        match crate::policy::gate(&ctx.root, policy, Date::today(), ctx.fix && !ctx.ci) {
            Ok(report) => {
                for line in report.status.to_string().lines() {
                    println!("    {line}");
                }
                for note in &report.notes {
                    println!("    note: {note}");
                }
                Self::report(&report.findings)
            }
            Err(error) => {
                println!(
                    "{}",
                    Finding {
                        rule: builtin::RECORD_INVALID,
                        path: policy.path.clone(),
                        line: 0,
                        message: error.to_string(),
                    }
                );
                Outcome::Failed
            }
        }
    }

    fn suppression(&self) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let record_path = &ctx.config.record.path;
        let record = Record::load(&ctx.root.join(record_path))?;
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
        let findings: Vec<Finding> = missing
            .into_iter()
            .map(|id| Finding {
                rule: builtin::DOCS_DANGLING,
                path: ctx.config.docs_dir.join(crate::docs::doc_file_name(&id)),
                line: 0,
                message: format!("`{id}` has no documentation"),
            })
            .collect();
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

    fn status(&self, cmd: &Cmd) -> Result<bool, Error> {
        println!("$ {}", cmd.display());
        let status = Command::new(&cmd.program)
            .args(&cmd.args)
            .current_dir(&self.ctx.root)
            .status()
            .map_err(|e| missing_tool(cmd, &e))?;
        Ok(status.success())
    }

    /// Runs every planned scan, so project and bundled findings are reported together.
    fn ast_grep(&self) -> Result<Outcome, Error> {
        let cmds = plan(Tool::AstGrep, &self.ctx);
        if cmds.is_empty() {
            return Ok(Outcome::Skipped(
                "no sgconfig.yml and the bundled pack is off".to_owned(),
            ));
        }
        if self.ctx.config.ast_grep.bundled {
            ast_grep::write_pack(&self.ctx.root)?;
        }
        let mut passed = true;
        for cmd in &cmds {
            passed &= self.status(cmd)?;
        }
        Ok(if passed {
            Outcome::Passed
        } else {
            Outcome::Failed
        })
    }

    fn scoped_diagnostics(&self, tool: Tool) -> Result<Outcome, Error> {
        let ctx = &self.ctx;
        let mut blocking = 0usize;
        let mut build_failed = false;
        for cmd in plan(tool, ctx) {
            println!("$ {}", cmd.display());
            let output = Command::new(&cmd.program)
                .args(&cmd.args)
                .current_dir(&ctx.root)
                .stderr(Stdio::inherit())
                .output()
                .map_err(|e| missing_tool(&cmd, &e))?;
            build_failed |= !output.status.success();
            for diagnostic in dedupe(parse_cargo_messages(&String::from_utf8_lossy(
                &output.stdout,
            ))) {
                if !in_scope(&diagnostic, &ctx.root, &ctx.scope) {
                    continue;
                }
                print!("{}", diagnostic.rendered);
                if let Some(code) = &diagnostic.code {
                    println!("  = doc: rust-qa rule-doc {code}");
                }
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
                "error: the build failed outside the scoped paths; scoped results are incomplete"
            );
            return Ok(Outcome::Failed);
        }
        Ok(Outcome::Passed)
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
    dylint_lister: fn(&Path) -> Result<Vec<DylintLint>, Error>,
) -> Result<String, Error> {
    let defences = enumerate(root, config, &dylint_lister)?;
    let record = Record::load(&root.join(&config.record.path))?;
    Ok(summary::render(&defences, &record, &config.record))
}
