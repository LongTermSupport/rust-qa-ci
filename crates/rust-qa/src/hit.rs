//! Findings of detectors that `rust-qa` applies the project record to itself: dylint lints, ast-grep
//! rules and project stages with `output = "findings"`. An exception with `covers = "findings"`
//! excepts such a finding at its path, with nothing written in the source.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::builtin::RECORD_STALE;
use crate::finding::Finding;
use crate::record::{Covers, Record, same_rule};

/// One finding of a detector, before or after the record is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Rule identifier, as the detector prints it.
    pub rule: String,
    /// File relative to the workspace root.
    pub path: PathBuf,
    /// One-based line, or zero when unknown.
    pub line: usize,
    /// One-based column, or zero when unknown.
    pub column: usize,
    /// What is wrong.
    pub message: String,
    /// How to put it right, when the detector says.
    pub note: Option<String>,
    /// The detector's own rendering (the compiler's, for a lint), printed in place of ours.
    pub rendered: Option<String>,
    /// Whether the finding fails the gate unless the record covers it.
    pub blocking: bool,
}

impl fmt::Display for Hit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(rendered) = &self.rendered {
            writeln!(f, "{}", rendered.trim_end())?;
        } else {
            let level = if self.blocking { "error" } else { "warning" };
            writeln!(f, "{level}[{}]: {}", self.rule, self.message)?;
            match (self.line, self.column) {
                (0, _) => writeln!(f, "  --> {}", self.path.display())?,
                (line, 0) => writeln!(f, "  --> {}:{line}", self.path.display())?,
                (line, column) => writeln!(f, "  --> {}:{line}:{column}", self.path.display())?,
            }
            if let Some(note) = &self.note {
                writeln!(f, "  = note: {note}")?;
            }
        }
        write!(f, "  = doc: rust-qa rule-doc {}", self.rule)
    }
}

/// Findings after the record is applied.
#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// Findings no exception covers, in their original order.
    pub kept: Vec<Hit>,
    /// How many blocking findings an exception covered.
    pub excepted: usize,
    /// Indexes of the exceptions that covered at least one finding, ascending.
    pub used: Vec<usize>,
}

impl Applied {
    /// How many kept findings fail the gate.
    #[must_use]
    pub fn blocking(&self) -> usize {
        self.kept.iter().filter(|hit| hit.blocking).count()
    }
}

/// Applies the record's `covers = "findings"` exceptions to blocking findings.
#[must_use]
pub fn apply(hits: Vec<Hit>, record: &Record) -> Applied {
    let mut applied = Applied::default();
    for hit in hits {
        let covered = if hit.blocking {
            record.find_finding(&hit.rule, &hit.path)
        } else {
            None
        };
        match covered {
            Some(index) => {
                applied.excepted += 1;
                if !applied.used.contains(&index) {
                    applied.used.push(index);
                }
            }
            None => applied.kept.push(hit),
        }
    }
    applied.used.sort_unstable();
    applied
}

/// `rqaci.record.stale` findings for the `covers = "findings"` exceptions whose rule is one of
/// `hosted` (the rules the detector that ran reports) and that covered nothing in a full run.
#[must_use]
pub fn unused(
    record: &Record,
    used: &[usize],
    hosted: &[String],
    record_path: &Path,
) -> Vec<Finding> {
    record
        .exceptions
        .iter()
        .enumerate()
        .filter(|(index, exception)| {
            exception.covers == Covers::Findings
                && !used.contains(index)
                && hosted.iter().any(|rule| same_rule(rule, &exception.rule))
        })
        .map(|(index, exception)| Finding {
            rule: RECORD_STALE,
            path: record_path.to_path_buf(),
            line: 0,
            message: format!(
                "exception #{} (rule `{}`, path `{}`, covers findings) matches no finding; remove it",
                index + 1,
                exception.rule,
                exception.path
            ),
        })
        .collect()
}
