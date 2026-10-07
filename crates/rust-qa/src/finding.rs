//! Diagnostics produced by the built-in detectors.

use std::fmt;
use std::path::PathBuf;

/// One violation of a built-in defence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable rule identifier, printed unaltered.
    pub rule: &'static str,
    /// Path relative to the workspace root.
    pub path: PathBuf,
    /// One-based line, or zero when the finding concerns the whole file.
    pub line: usize,
    /// Terse description of the violation.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "error[{}]: {}", self.rule, self.message)?;
        if self.line == 0 {
            writeln!(f, "  --> {}", self.path.display())?;
        } else {
            writeln!(f, "  --> {}:{}", self.path.display(), self.line)?;
        }
        write!(f, "  = doc: rust-qa rule-doc {}", self.rule)
    }
}
