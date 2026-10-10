//! Project configuration: built-in defaults overridden by `qa/rust-qa.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::Error;
use crate::policy::RecordPolicy;
use crate::stage::Stage;

/// Location of the project configuration file, relative to the workspace root.
pub const DEFAULT_PATH: &str = "qa/rust-qa.toml";

/// Effective configuration after the cascade.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Project record of exceptions and its anti-bloat policy: either `record = "<path>"` or a
    /// `[record]` table.
    #[serde(deserialize_with = "record_setting")]
    pub record: RecordPolicy,
    /// Directory of project-authored rule documents (`<id>.md`).
    pub docs_dir: PathBuf,
    /// Globs (relative to the root) excluded from source scans (the suppression guard and the
    /// bundled ast-grep pack), in addition to `target` and `.git` directories, which are always
    /// skipped.
    pub scan_exclude: Vec<String>,
    /// Agent summary settings.
    pub summary: Summary,
    /// Tool switches.
    pub tools: Tools,
    /// ast-grep detector settings.
    pub ast_grep: AstGrep,
    /// Coverage gate.
    pub coverage: Coverage,
    /// Mutation testing policy.
    pub mutants: Mutants,
    /// Further Cargo workspaces in the repository (directories relative to the root), such as a
    /// dylint library, which keeps a workspace and a pinned toolchain of its own. `fmt`, `clippy`,
    /// `dylint` and `test` check each one, in its directory and with the toolchain its own
    /// `rust-toolchain.toml` names.
    pub workspaces: Vec<PathBuf>,
    /// dylint detector settings.
    pub dylint: Dylint,
    /// Project stages: further commands the gate runs, each in a phase.
    #[serde(rename = "stage")]
    pub stages: Vec<Stage>,
    /// Documentation for identifiers a tool prints but nothing lists, such as the codes of a
    /// linter a stage runs, matched by glob.
    #[serde(rename = "resolver")]
    pub resolvers: Vec<DocResolver>,
}

/// Documentation for the identifiers matching a glob: a text with `{id}` for the identifier, such
/// as where the tool's own pages are.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocResolver {
    /// Glob over the identifier, such as `SC[0-9]*`.
    pub matches: String,
    /// The documentation printed, with `{id}` replaced.
    pub text: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            record: RecordPolicy::default(),
            docs_dir: PathBuf::from("docs/defences"),
            scan_exclude: Vec::new(),
            summary: Summary::default(),
            tools: Tools::default(),
            ast_grep: AstGrep::default(),
            coverage: Coverage::default(),
            mutants: Mutants::default(),
            workspaces: Vec::new(),
            dylint: Dylint::default(),
            stages: Vec::new(),
            resolvers: Vec::new(),
        }
    }
}

/// dylint detector settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Dylint {
    /// Force every lint of the configured libraries to warn (`--force-warn`), so that no level
    /// written in the code can lower or silence one; `rust-qa` then fails the gate on each finding
    /// the record does not cover.
    pub force_warn: bool,
}

/// Where the generated agent summary region is written.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Summary {
    /// Agent-loaded file holding the generated region; `None` disables the summary check.
    pub file: Option<PathBuf>,
}

/// Per-tool switches. A tool whose own configuration file is absent reports itself as skipped.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one switch per tool mirrors the configuration file one to one"
)]
pub struct Tools {
    /// `cargo fmt`.
    pub fmt: bool,
    /// Project record validation.
    pub record: bool,
    /// Suppression guard.
    pub suppression: bool,
    /// Agent summary freshness.
    pub summary: bool,
    /// Every listed identifier resolves to documentation.
    pub docs: bool,
    /// ast-grep supplementary syntactic rules.
    pub ast_grep: bool,
    /// Clippy at deny.
    pub clippy: bool,
    /// dylint type-aware rules.
    pub dylint: bool,
    /// cargo-deny.
    pub deny: bool,
    /// cargo-audit.
    pub audit: bool,
    /// cargo-machete.
    pub machete: bool,
    /// Tests (nextest plus doctests).
    pub test: bool,
    /// Coverage gate.
    pub coverage: bool,
    /// Mutation testing.
    pub mutants: bool,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            fmt: true,
            record: true,
            suppression: true,
            summary: true,
            docs: true,
            ast_grep: true,
            clippy: true,
            dylint: true,
            deny: true,
            audit: false,
            machete: true,
            test: true,
            coverage: true,
            mutants: true,
        }
    }
}

/// ast-grep detector settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AstGrep {
    /// Run the bundled rule pack (`rqaci.ast.*`) in addition to the project's `sgconfig.yml`
    /// rules.
    pub bundled: bool,
}

impl Default for AstGrep {
    fn default() -> Self {
        Self { bundled: true }
    }
}

/// Coverage gate settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Coverage {
    /// Minimum line coverage percentage.
    pub fail_under_lines: u8,
}

impl Default for Coverage {
    fn default() -> Self {
        Self {
            fail_under_lines: 80,
        }
    }
}

/// Mutation testing settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mutants {
    /// Which code is mutated.
    pub mode: MutantsMode,
    /// Git base for `diff` mode.
    pub base: String,
}

impl Default for Mutants {
    fn default() -> Self {
        Self {
            mode: MutantsMode::Off,
            base: "origin/main".to_owned(),
        }
    }
}

/// Which code cargo-mutants mutates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MutantsMode {
    /// Not run.
    #[default]
    Off,
    /// Only code changed relative to the base.
    Diff,
    /// The whole workspace.
    Full,
}

impl Config {
    /// Loads the configuration for `root`.
    ///
    /// Without `explicit`, `qa/rust-qa.toml` is read when present and defaults apply otherwise.
    ///
    /// # Errors
    /// Returns an error when the file is unreadable, invalid, names unknown keys, or sets a coverage
    /// threshold above 100.
    pub fn load(root: &Path, explicit: Option<&Path>) -> Result<Self, Error> {
        let path = if let Some(path) = explicit {
            root.join(path)
        } else {
            let path = root.join(DEFAULT_PATH);
            if !path.exists() {
                return Ok(Self::default());
            }
            path
        };
        let config: Self = crate::read_toml(&path)?;
        if config.coverage.fail_under_lines > 100 {
            return Err(Error::Usage(format!(
                "{}: coverage.fail_under_lines must be at most 100",
                path.display()
            )));
        }
        config
            .record
            .validate()
            .map_err(|problems| Error::Usage(format!("{}: [record] {problems}", path.display())))?;
        let mut problems = Vec::new();
        for (index, stage) in config.stages.iter().enumerate() {
            problems.extend(stage.problems());
            let earlier = config.stages.iter().take(index);
            if earlier.into_iter().any(|other| other.name == stage.name) {
                problems.push(format!("stage `{}` is declared twice", stage.name));
            }
            if crate::tools::Tool::from_name(&stage.name).is_some() {
                problems.push(format!(
                    "stage `{}` has the name of a built-in tool",
                    stage.name
                ));
            }
        }
        for resolver in &config.resolvers {
            if let Err(error) = globset::Glob::new(&resolver.matches) {
                problems.push(format!("resolver `{}`: {error}", resolver.matches));
            }
            if resolver.text.trim().is_empty() {
                problems.push(format!("resolver `{}` has no text", resolver.matches));
            }
        }
        for workspace in &config.workspaces {
            if !root.join(workspace).join("Cargo.toml").is_file() {
                problems.push(format!(
                    "workspace `{}` has no Cargo.toml",
                    workspace.display()
                ));
            }
        }
        if !problems.is_empty() {
            return Err(Error::Usage(format!(
                "{}: {}",
                path.display(),
                problems.join("; ")
            )));
        }
        Ok(config)
    }
}

/// `record = "<path>"` (defaults for everything else) or a `[record]` table.
fn record_setting<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RecordPolicy, D::Error> {
    match toml::Value::deserialize(deserializer)? {
        toml::Value::String(path) => Ok(RecordPolicy {
            path: PathBuf::from(path),
            ..RecordPolicy::default()
        }),
        table @ toml::Value::Table(_) => table.try_into().map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!(
            "`record` must be a path or a table, found {}",
            other.type_str()
        ))),
    }
}
