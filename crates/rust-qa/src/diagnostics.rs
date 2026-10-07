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
}

impl Diagnostic {
    /// Whether the diagnostic fails the build.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.level.starts_with("error")
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
            let files = message
                .get("spans")
                .and_then(Value::as_array)
                .map(|spans| {
                    spans
                        .iter()
                        .filter(|span| {
                            span.get("is_primary").and_then(Value::as_bool) == Some(true)
                        })
                        .filter_map(|span| span.get("file_name").and_then(Value::as_str))
                        .map(PathBuf::from)
                        .collect()
                })
                .unwrap_or_default();
            Diagnostic {
                code: message
                    .get("code")
                    .and_then(|code| code.get("code"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                level: text("level"),
                rendered: text("rendered"),
                files,
            }
        })
        .collect()
}

/// Whether any primary span of `diagnostic` lies in one of the `scope` paths (relative to `root`).
#[must_use]
pub fn in_scope(diagnostic: &Diagnostic, root: &Path, scope: &[PathBuf]) -> bool {
    diagnostic.files.iter().any(|file| {
        let relative = file.strip_prefix(root).unwrap_or(file);
        scope.iter().any(|wanted| relative.starts_with(wanted))
    })
}
