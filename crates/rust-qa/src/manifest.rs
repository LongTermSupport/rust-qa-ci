//! Cargo manifest inspection and workspace file walking.

use std::path::{Path, PathBuf};

use globset::GlobSet;
use toml::Value;

use crate::Error;
use crate::record::normalise_rule;

/// Lint level as written in a `[lints]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Silences the lint.
    Allow,
    /// Reports and continues (blocking under `-D warnings`).
    Warn,
    /// Fails the build.
    Deny,
    /// Fails the build and cannot be lowered in source.
    Forbid,
}

impl Level {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "allow" => Some(Self::Allow),
            "warn" => Some(Self::Warn),
            "deny" => Some(Self::Deny),
            "forbid" => Some(Self::Forbid),
            _ => None,
        }
    }

    /// Lower-case name as written in manifests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Warn => "warn",
            Self::Deny => "deny",
            Self::Forbid => "forbid",
        }
    }
}

/// One entry of a `[lints.<tool>]` or `[workspace.lints.<tool>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintSetting {
    /// Identifier as the compiler prints it (`clippy::unwrap_used`, `unsafe_code`).
    pub id: String,
    /// Configured level.
    pub level: Level,
    /// Manifest path relative to the root.
    pub manifest: PathBuf,
    /// One-based line of the entry.
    pub line: usize,
}

/// Reads every lint setting from a manifest's text, skipping `workspace = true` inheritance.
///
/// # Errors
/// Returns an error when the text is not TOML or a level is unknown.
pub fn lint_settings(text: &str, manifest: &Path) -> Result<Vec<LintSetting>, Error> {
    let value: Value = toml::from_str(text).map_err(|source| Error::Toml {
        path: manifest.to_path_buf(),
        source: Box::new(source),
    })?;
    let mut settings = Vec::new();
    let tables = [
        value.get("workspace").and_then(|w| w.get("lints")),
        value.get("lints"),
    ];
    for lints in tables.into_iter().flatten() {
        let Some(lints) = lints.as_table() else {
            continue;
        };
        for (tool, entries) in lints {
            let Some(entries) = entries.as_table() else {
                continue;
            };
            for (name, setting) in entries {
                let level_text = match setting {
                    Value::String(level) => level.as_str(),
                    Value::Table(table) => table.get("level").and_then(Value::as_str).unwrap_or(""),
                    _ => "",
                };
                let level = Level::parse(level_text).ok_or_else(|| {
                    Error::Usage(format!(
                        "{}: lint `{tool}.{name}` has unknown level `{level_text}`",
                        manifest.display()
                    ))
                })?;
                let id = if tool == "rust" {
                    normalise_rule(name)
                } else {
                    normalise_rule(&format!("{tool}::{name}"))
                };
                settings.push(LintSetting {
                    id,
                    level,
                    manifest: manifest.to_path_buf(),
                    line: line_of_key(text, tool, name),
                });
            }
        }
    }
    Ok(settings)
}

/// Line of `name = ...` inside the `[...lints.<tool>]` table, or zero when not found.
fn line_of_key(text: &str, tool: &str, name: &str) -> usize {
    let mut in_table = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let header = trimmed.trim_matches(|c| c == '[' || c == ']').trim();
            in_table =
                header == format!("lints.{tool}") || header == format!("workspace.lints.{tool}");
            continue;
        }
        if in_table {
            let key = trimmed
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"');
            if key == name {
                return index + 1;
            }
        }
    }
    0
}

/// Whether the root manifest declares `[workspace.metadata.dylint]` libraries.
///
/// # Errors
/// Returns an error when the root manifest cannot be read or parsed.
pub fn has_dylint_metadata(root: &Path) -> Result<bool, Error> {
    let value: Value = crate::read_toml(&root.join("Cargo.toml"))?;
    Ok(value
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("dylint"))
        .and_then(|d| d.get("libraries"))
        .is_some())
}

/// Files under `root` (relative paths, sorted) accepted by `keep`. Honours `.gitignore` files (so
/// scratch clones and build output are skipped), always skips `target` and `.git` directories, keeps
/// hidden configuration directories such as `.cargo`, and drops anything matching `exclude`. A
/// non-empty `scope` limits the walk to those files or directories (relative to `root`).
#[must_use]
pub fn workspace_files(
    root: &Path,
    scope: &[PathBuf],
    exclude: &GlobSet,
    keep: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let starts: Vec<PathBuf> = if scope.is_empty() {
        vec![root.to_path_buf()]
    } else {
        scope.iter().map(|s| root.join(s)).collect()
    };
    let mut files = Vec::new();
    for start in starts {
        let walker = ignore::WalkBuilder::new(&start)
            .hidden(false)
            .parents(false)
            .git_global(false)
            .require_git(false)
            .filter_entry(|entry| {
                let name = entry.file_name();
                let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                !(is_dir && entry.depth() > 0 && (name == "target" || name == ".git"))
            })
            .build();
        for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(root) else {
                continue;
            };
            if exclude.is_match(rel) || !keep(rel) {
                continue;
            }
            files.push(rel.to_path_buf());
        }
    }
    files.sort();
    files.dedup();
    files
}
