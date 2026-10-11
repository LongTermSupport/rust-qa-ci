//! Active defence enumeration, derived from the configuration in force (Cargo `[lints]` tables,
//! dylint libraries, ast-grep rule directories and the bundled ast-grep pack, `qa/rust-qa.toml` and
//! its project stages, with the rules those stages report), never from a hand-kept list.

use std::path::{Path, PathBuf};
use std::process::Command;

use yaml_rust2::{Yaml, YamlLoader};

use crate::config::{Config, MutantsMode};
use crate::manifest::{Level, has_dylint_metadata, lint_settings, workspace_files};
use crate::stage::{Stage, StageRule, parse_rules};
use crate::{Error, ast_grep, builtin, read_to_string};

/// What enforces a defence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Engine {
    /// Implemented inside `rust-qa`.
    Builtin,
    /// rustc lint.
    Rustc,
    /// Clippy lint.
    Clippy,
    /// rustdoc lint.
    Rustdoc,
    /// dylint library lint (type-aware).
    Dylint,
    /// ast-grep rule (syntactic).
    AstGrep,
    /// Runner gate (executes code or audits dependencies); not a static detector.
    Runner,
    /// A project stage, or a rule one reports.
    Stage,
}

impl Engine {
    /// Lower-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Builtin => "rust-qa",
            Self::Rustc => "rustc",
            Self::Clippy => "clippy",
            Self::Rustdoc => "rustdoc",
            Self::Dylint => "dylint",
            Self::AstGrep => "ast-grep",
            Self::Runner => "runner",
            Self::Stage => "stage",
        }
    }
}

/// One active defence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defence {
    /// Identifier exactly as printed on failure.
    pub id: String,
    /// Enforcing engine.
    pub engine: Engine,
    /// Effective level or gate setting.
    pub level: String,
    /// Where it is configured.
    pub origin: String,
    /// What it guards, where the configuration says (empty otherwise).
    pub description: String,
    /// Whether the agent summary lists it.
    pub summarised: bool,
    /// How to fix what it finds, from its project page, for the summary (empty otherwise).
    pub fix: String,
}

/// A lint reported by `cargo dylint list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DylintLint {
    /// Library name.
    pub library: String,
    /// Lint name.
    pub name: String,
    /// Default level.
    pub level: String,
    /// What the lint finds.
    pub description: String,
}

/// An ast-grep rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstGrepRule {
    /// Rule id.
    pub id: String,
    /// Severity (`error` blocks).
    pub severity: String,
    /// Rule file relative to the root.
    pub file: PathBuf,
    /// The language the rule reads, as the rule spells it.
    pub language: String,
    /// The rule's message.
    pub message: String,
}

/// Lists every active defence for the workspace at `root`.
///
/// `dylint` lists the lints of the configured dylint libraries; the real implementation is
/// [`dylint_lints`].
///
/// # Errors
/// Returns an error when a configuration file cannot be read or parsed, or dylint listing fails.
pub fn enumerate(
    root: &Path,
    config: &Config,
    dylint: &dyn Fn(&Path) -> Result<Vec<DylintLint>, Error>,
) -> Result<Vec<Defence>, Error> {
    let mut out = Collector::default();
    let tools = &config.tools;
    let builtin = |out: &mut Collector, id: &str, origin: &str| {
        out.push(id, Engine::Builtin, "deny", origin, builtin::describe(id));
    };
    if tools.record {
        for id in [
            builtin::RECORD_INVALID,
            builtin::RECORD_BUDGET,
            builtin::RECORD_RATCHET,
            builtin::RECORD_EXPIRED,
        ] {
            builtin(&mut out, id, "rust-qa record");
        }
    }
    if tools.suppression {
        for id in [
            builtin::SUPPRESSION_UNRECORDED,
            builtin::SUPPRESSION_ALLOW,
            builtin::RECORD_STALE,
        ] {
            builtin(&mut out, id, "rust-qa suppression");
        }
    }
    if tools.summary && config.summary.file.is_some() {
        builtin(&mut out, builtin::SUMMARY_STALE, "rust-qa summary");
    }
    if tools.docs {
        builtin(&mut out, builtin::DOCS_DANGLING, "rust-qa docs");
        builtin(&mut out, builtin::DOCS_ORPHAN, "rust-qa docs");
    }
    if tools.clippy {
        out.push(
            "warnings",
            Engine::Rustc,
            "deny (-D warnings)",
            "rust-qa clippy",
            "every compiler and Clippy warning fails the gate",
        );
        lint_tables(root, config, &mut out)?;
    }
    if tools.dylint && has_dylint_metadata(root)? {
        dylint_defences(config, dylint(root)?, &mut out);
    }
    if tools.ast_grep && root.join("sgconfig.yml").is_file() {
        let rules = ast_grep_rules(root)?;
        if !rules.is_empty() {
            builtin(&mut out, builtin::RULES_UNTESTED, "rust-qa ast-grep");
        }
        for rule in rules {
            if rule.severity == "error" {
                out.push(
                    &rule.id,
                    Engine::AstGrep,
                    "error",
                    &rule.file.display().to_string(),
                    &terse_message(&rule.id, &rule.message),
                );
            }
        }
    }
    if tools.ast_grep && config.ast_grep.bundled {
        for rule in ast_grep::bundled() {
            out.push(
                rule.id,
                Engine::AstGrep,
                "error",
                "rust-qa bundled ast-grep pack",
                &ast_grep::message(rule),
            );
        }
    }
    stage_defences(root, config, &mut out)?;
    runners(root, config, &mut out);
    Ok(out.defences)
}

fn dylint_defences(config: &Config, lints: Vec<DylintLint>, out: &mut Collector) {
    for lint in lints {
        let level = if config.dylint.force_warn {
            "forced; every finding blocks".to_owned()
        } else {
            lint.level.clone()
        };
        let origin = format!("dylint library {}", lint.library);
        out.push(
            &lint.name,
            Engine::Dylint,
            &level,
            &origin,
            &lint.description,
        );
    }
}

fn stage_defences(root: &Path, config: &Config, out: &mut Collector) -> Result<(), Error> {
    for stage in &config.stages {
        out.push(
            &stage.name,
            Engine::Stage,
            stage.phase.name(),
            "qa/rust-qa.toml [[stage]]",
            &stage.says,
        );
        if stage.rules.is_some() {
            let origin = format!("stage {}", stage.name);
            for rule in stage_rules(root, stage)? {
                out.push(&rule.id, Engine::Stage, "error", &origin, &rule.description);
                if let Some(listed) = out.defences.iter_mut().find(|d| d.id == rule.id) {
                    listed.summarised &= stage.summary_rules;
                }
            }
        }
    }
    Ok(())
}

/// A rule's message without its own identifier in front, which the listing shows already, or a
/// trailing `(page.md)` reference.
#[must_use]
pub fn terse_message(id: &str, message: &str) -> String {
    let message = message.trim();
    let message = message
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix(':'))
        .map_or(message, str::trim_start);
    let message = match message.rfind(" (") {
        Some(at) if message.ends_with(".md)") => message.get(..at).unwrap_or(message),
        _ => message,
    };
    message.trim().to_owned()
}

/// The rules a project stage reports, from its `rules` command (none without one).
///
/// # Errors
/// Returns an error when the command cannot be run, fails, or prints something other than a JSON
/// list of rules.
pub fn stage_rules(root: &Path, stage: &Stage) -> Result<Vec<StageRule>, Error> {
    let Some((program, args)) = stage.rules.as_ref().and_then(|argv| argv.split_first()) else {
        return Ok(Vec::new());
    };
    let line = format!("{program} {}", args.join(" "));
    let cmd = crate::tools::Cmd {
        program: program.clone(),
        args: args.to_vec(),
        dir: stage.dir.clone(),
        own_toolchain: stage.dir.is_some(),
        env: Vec::new(),
    };
    let output = cmd
        .command()
        .current_dir(
            stage
                .dir
                .as_ref()
                .map_or_else(|| root.to_path_buf(), |d| root.join(d)),
        )
        .output()
        .map_err(|e| Error::Command {
            command: line.clone(),
            message: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Command {
            command: line,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    parse_rules(&String::from_utf8_lossy(&output.stdout)).map_err(|message| Error::Command {
        command: line,
        message: format!("printed no rule list: {message}"),
    })
}

/// Accumulates defences, keeping the first entry for each identifier.
#[derive(Default)]
struct Collector {
    defences: Vec<Defence>,
}

impl Collector {
    fn push(&mut self, id: &str, engine: Engine, level: &str, origin: &str, description: &str) {
        if self.defences.iter().all(|d| d.id != id) {
            self.defences.push(Defence {
                id: id.to_owned(),
                engine,
                level: level.to_owned(),
                origin: origin.to_owned(),
                description: description.to_owned(),
                summarised: true,
                fix: String::new(),
            });
        }
    }
}

fn lint_tables(root: &Path, config: &Config, out: &mut Collector) -> Result<(), Error> {
    let exclude = crate::glob_set(&config.scan_exclude)?;
    let manifests = workspace_files(root, &[], &exclude, |p| {
        p.file_name().is_some_and(|n| n == "Cargo.toml")
    });
    for manifest in manifests {
        let text = read_to_string(&root.join(&manifest))?;
        for setting in lint_settings(&text, &manifest)? {
            let engine = if setting.id.starts_with("clippy::") {
                Engine::Clippy
            } else if setting.id.starts_with("rustdoc::") {
                Engine::Rustdoc
            } else {
                Engine::Rustc
            };
            let level = match setting.level {
                Level::Allow => continue,
                Level::Warn => "warn (blocking via -D warnings)",
                other => other.as_str(),
            };
            let origin = format!("{}:{}", setting.manifest.display(), setting.line);
            out.push(&setting.id, engine, level, &origin, "");
        }
    }
    Ok(())
}

fn runners(root: &Path, config: &Config, out: &mut Collector) {
    let tools = &config.tools;
    if tools.deny && root.join("deny.toml").is_file() {
        out.push(
            "cargo-deny",
            Engine::Runner,
            "advisories, bans, licenses, sources",
            "deny.toml",
            "",
        );
    }
    if tools.audit {
        out.push(
            "cargo-audit",
            Engine::Runner,
            "advisories",
            "rust-qa audit",
            "",
        );
    }
    if tools.machete {
        out.push(
            "cargo-machete",
            Engine::Runner,
            "unused dependencies",
            "rust-qa machete",
            "",
        );
    }
    if tools.test {
        out.push(
            "tests",
            Engine::Runner,
            "nextest plus doctests",
            "rust-qa test",
            "",
        );
    }
    if tools.coverage {
        let level = format!("lines >= {}%", config.coverage.fail_under_lines);
        out.push(
            "coverage",
            Engine::Runner,
            &level,
            "qa/rust-qa.toml [coverage]",
            "",
        );
    }
    if tools.mutants && config.mutants.mode != MutantsMode::Off {
        let level = if config.mutants.mode == MutantsMode::Diff {
            format!(
                "missed mutants fail, code changed since {}",
                config.mutants.base
            )
        } else {
            "missed mutants fail, whole workspace".to_owned()
        };
        out.push(
            "mutants",
            Engine::Runner,
            &level,
            "qa/rust-qa.toml [mutants]",
            "",
        );
    }
}

/// Parses `cargo dylint list --all` output: unindented library names followed by indented
/// `name level description` lines.
#[must_use]
pub fn parse_dylint_list(output: &str) -> Vec<DylintLint> {
    let mut library = String::new();
    let mut lints = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            let mut columns = line.split_whitespace();
            if let (Some(name), Some(level)) = (columns.next(), columns.next()) {
                lints.push(DylintLint {
                    library: library.clone(),
                    name: name.to_owned(),
                    level: level.to_owned(),
                    description: columns.collect::<Vec<_>>().join(" "),
                });
            }
        } else {
            line.trim().clone_into(&mut library);
        }
    }
    lints
}

/// Lints of every library in `[workspace.metadata.dylint]`, via `cargo dylint list --all` (builds
/// the libraries when needed; does not lint the workspace).
///
/// # Errors
/// Returns an error when cargo-dylint is missing or fails.
pub fn dylint_lints(root: &Path) -> Result<Vec<DylintLint>, Error> {
    let command = "cargo dylint list --all";
    let output = Command::new("cargo")
        .args(["dylint", "list", "--all"])
        .current_dir(root)
        .output()
        .map_err(|e| Error::Command {
            command: command.to_owned(),
            message: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(Error::Command {
            command: command.to_owned(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(parse_dylint_list(&String::from_utf8_lossy(&output.stdout)))
}

/// Rules found in the `ruleDirs` of `sgconfig.yml`, in file order.
///
/// # Errors
/// Returns an error when a YAML file cannot be read or parsed.
pub fn ast_grep_rules(root: &Path) -> Result<Vec<AstGrepRule>, Error> {
    let config_path = root.join("sgconfig.yml");
    let docs = load_yaml(&config_path)?;
    let dirs: Vec<PathBuf> = docs
        .first()
        .and_then(|doc| field(doc, "ruleDirs"))
        .and_then(Yaml::as_vec)
        .map(|dirs| {
            dirs.iter()
                .filter_map(Yaml::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    let mut rules = Vec::new();
    let none = crate::glob_set::<&str>(&[])?;
    for dir in dirs {
        let files = workspace_files(root, std::slice::from_ref(&dir), &none, |p| {
            p.extension().is_some_and(|e| e == "yml" || e == "yaml")
        });
        for file in files {
            for doc in load_yaml(&root.join(&file))? {
                if let Some(id) = field(&doc, "id").and_then(Yaml::as_str) {
                    rules.push(AstGrepRule {
                        id: id.to_owned(),
                        severity: field(&doc, "severity")
                            .and_then(Yaml::as_str)
                            .unwrap_or("hint")
                            .to_owned(),
                        file: file.clone(),
                        language: field(&doc, "language")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        message: field(&doc, "message")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_owned(),
                    });
                }
            }
        }
    }
    Ok(rules)
}

/// The file-name globs `languageGlobs` in `sgconfig.yml` adds to each language, as spelled there.
///
/// # Errors
/// Returns an error when `sgconfig.yml` cannot be read or parsed.
pub fn language_globs(root: &Path) -> Result<Vec<(String, Vec<String>)>, Error> {
    let docs = load_yaml(&root.join("sgconfig.yml"))?;
    Ok(docs
        .first()
        .and_then(|doc| field(doc, "languageGlobs"))
        .and_then(Yaml::as_hash)
        .map(|table| {
            table
                .iter()
                .filter_map(|(language, globs)| {
                    let globs = match globs {
                        Yaml::Array(items) => items
                            .iter()
                            .filter_map(Yaml::as_str)
                            .map(ToOwned::to_owned)
                            .collect(),
                        Yaml::String(one) => vec![one.clone()],
                        _ => Vec::new(),
                    };
                    language.as_str().map(|name| (name.to_owned(), globs))
                })
                .collect()
        })
        .unwrap_or_default())
}

pub(crate) fn field<'a>(doc: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    doc.as_hash()?.get(&Yaml::String(key.to_owned()))
}

pub(crate) fn load_yaml(path: &Path) -> Result<Vec<Yaml>, Error> {
    let text = read_to_string(path)?;
    YamlLoader::load_from_str(&text).map_err(|e| Error::Yaml {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}
