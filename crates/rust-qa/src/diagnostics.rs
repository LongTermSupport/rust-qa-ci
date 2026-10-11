//! Cargo `--message-format=json` diagnostics, used to narrow per-crate tools (Clippy, dylint) to a
//! file subset.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One compiler diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Lint or error code, printed unaltered (`clippy::unwrap_used`, `E0308`).
    pub code: Option<String>,
    /// `error`, `warning`, `note`, ...
    pub level: String,
    /// Human-readable rendering from the compiler.
    pub rendered: String,
    /// Files of the primary spans.
    pub files: Vec<PathBuf>,
    /// The message alone, without the rendering's source excerpt.
    pub message: String,
    /// One-based line of the first primary span, or zero.
    pub line: usize,
    /// One-based column of the first primary span, or zero.
    pub column: usize,
}

impl Diagnostic {
    /// Whether the diagnostic fails the build.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.level.starts_with("error")
    }

    /// The diagnostic with its files named from `root`, for cargo run in the workspace at
    /// `prefix` (relative to `root`, empty for the root itself), which names them from there.
    #[must_use]
    pub fn under(mut self, root: &Path, prefix: &Path) -> Self {
        self.files = self
            .files
            .into_iter()
            .map(|file| {
                let file = if file.is_absolute() {
                    file
                } else {
                    prefix.join(file)
                };
                file.strip_prefix(root)
                    .map_or(file.clone(), Path::to_path_buf)
            })
            .collect();
        self
    }
}

/// Extracts compiler messages from a cargo JSON message stream, ignoring other lines.
#[must_use]
pub fn parse_cargo_messages(stream: &str) -> Vec<Diagnostic> {
    stream
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("reason").and_then(Value::as_str) == Some("compiler-message"))
        .filter_map(|value| value.get("message").cloned())
        .map(|message| {
            let text = |key: &str| {
                message
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            };
            let primary: Vec<&Value> = message
                .get("spans")
                .and_then(Value::as_array)
                .map(|spans| {
                    spans
                        .iter()
                        .filter(|span| {
                            span.get("is_primary").and_then(Value::as_bool) == Some(true)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let files = primary
                .iter()
                .filter_map(|span| span.get("file_name").and_then(Value::as_str))
                .map(PathBuf::from)
                .collect();
            let position = |key: &str| {
                primary
                    .first()
                    .and_then(|span| span.get(key))
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0)
            };
            Diagnostic {
                code: message
                    .get("code")
                    .and_then(|code| code.get("code"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                level: text("level"),
                rendered: text("rendered"),
                files,
                message: text("message"),
                line: position("line_start"),
                column: position("column_start"),
            }
        })
        .collect()
}

/// Drops repeats of the same diagnostic (a crate checked once per target reports each finding once per
/// target), keeping first-seen order.
#[must_use]
pub fn dedupe(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen: Vec<Diagnostic> = Vec::new();
    for diagnostic in diagnostics {
        if !seen.contains(&diagnostic) {
            seen.push(diagnostic);
        }
    }
    seen
}

/// Whether any primary span of `diagnostic` lies in one of the `scope` paths (relative to `root`).
#[must_use]
pub fn in_scope(diagnostic: &Diagnostic, root: &Path, scope: &[PathBuf]) -> bool {
    diagnostic.files.iter().any(|file| {
        let relative = file.strip_prefix(root).unwrap_or(file);
        scope.iter().any(|wanted| relative.starts_with(wanted))
    })
}
