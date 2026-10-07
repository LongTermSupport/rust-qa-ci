//! Project configuration: built-in defaults overridden by `qa/rust-qa.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::Error;
use crate::policy::RecordPolicy;

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
        }
    }
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
