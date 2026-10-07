//! The project record: the one place exceptions to a defence are decided, each with a written
//! justification.
//!
//! File format (`qa/record.toml` by default):
//!
//! ```toml
//! [ratchet]                      # ceiling on the entry count; see `crate::policy`
//! high_water = 1
//! decided_by = "owner"
//! decided_on = 2026-01-15
//! reason = "why the ceiling is at this value"
//!
//! [[exception]]
//! rule = "clippy::unwrap_used"   # identifier exactly as the defence prints it
//! path = "src/parser.rs"         # one file; globs need `policy = true`
//! justification = "why the rule is wrong here and what makes the exception safe"
//! decided_by = "owner"
//! added_by_model = "human"       # or the model id that wrote the entry
//! decided_on = 2026-01-15
//! review_by = 2026-04-15
//! ```
//!
//! This module checks structure and wording; budgets, deadlines, trust tiers and the ratchet are
//! checked by [`crate::policy`].

use std::path::Path;

use serde::Deserialize;

use crate::Error;
use crate::builtin::RECORD_INVALID;
use crate::date::Date;
use crate::finding::Finding;

/// Justifications shorter than this many characters are rejected as not explaining anything.
pub const MIN_JUSTIFICATION_CHARS: usize = 25;

/// Whole-text justifications that say nothing (compared after lower-casing and trimming punctuation).
const GENERIC: [&str; 22] = [
    "todo",
    "tbd",
    "fixme",
    "legacy",
    "legacy code",
    "needed",
    "needed for now",
    "required",
    "temporary",
    "temp",
    "fix later",
    "later",
    "n/a",
    "na",
    "none",
    "ignore",
    "wip",
    "hack",
    "workaround",
    "false positive",
    "see above",
    "as above",
];

/// Placeholder words rejected anywhere in a justification.
const PLACEHOLDERS: [&str; 4] = ["todo", "tbd", "fixme", "xxx"];

/// Parsed project record.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Signed ceiling on the number of exceptions.
    #[serde(default)]
    pub ratchet: Option<Ratchet>,
    /// Exceptions in file order.
    #[serde(default, rename = "exception")]
    pub exceptions: Vec<Exception>,
}

/// The committed ceiling on the entry count: lowered automatically, raised only by a signed
/// decision.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ratchet {
    /// Maximum number of exceptions.
    pub high_water: usize,
    /// Human (from the configured deciders) who set the ceiling.
    pub decided_by: String,
    /// When the ceiling was last raised or adopted.
    pub decided_on: Date,
    /// Why the ceiling is at this value.
    pub reason: String,
}

/// One decided exception.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// Rule identifier as printed by the defence.
    pub rule: String,
    /// File or glob relative to the workspace root.
    pub path: String,
    /// Why the exception is correct.
    pub justification: String,
    /// Who decided it (required by the policy check).
    #[serde(default)]
    pub decided_by: Option<String>,
    /// Model id that wrote the entry, or `human` (required by the policy check).
    #[serde(default)]
    pub added_by_model: Option<String>,
    /// When it was decided (required by the policy check).
    #[serde(default)]
    pub decided_on: Option<Date>,
    /// When it must be re-decided or removed (required by the policy check).
    #[serde(default)]
    pub review_by: Option<Date>,
    /// Marks a deliberately broad (glob) exception, which has its own, stricter cap.
    #[serde(default)]
    pub policy: bool,
}

impl Record {
    /// Loads the record; a missing file is an empty record.
    ///
    /// # Errors
    /// Returns an error when the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.exists() {
            return Ok(Self::default());
        }
        crate::read_toml(path)
    }

    /// Checks every exception for a rule, a valid path glob, an explicit decider when given, and a
    /// specific justification.
    #[must_use]
    pub fn validate(&self, record_path: &Path) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (index, exception) in self.exceptions.iter().enumerate() {
            let label = format!("exception #{} (rule `{}`)", index + 1, exception.rule);
            let mut problems = Vec::new();
            if exception.rule.trim().is_empty() {
                problems.push("has no rule".to_owned());
            }
            if exception.path.trim().is_empty() {
                problems.push("has no path".to_owned());
            } else if globset::Glob::new(&exception.path).is_err() {
                problems.push(format!("has an invalid path glob `{}`", exception.path));
            }
            if exception
                .decided_by
                .as_ref()
                .is_some_and(|who| who.trim().is_empty())
            {
                problems.push("has an empty decided_by".to_owned());
            }
            if let Some(problem) = justification_problem(&exception.justification) {
                problems.push(format!("justification {problem}"));
            }
            if !problems.is_empty() {
                findings.push(Finding {
                    rule: RECORD_INVALID,
                    path: record_path.to_path_buf(),
                    line: 0,
                    message: format!("{label} {}", problems.join("; ")),
                });
            }
        }
        findings
    }

    /// Index of the first exception covering `rule` at `path` (relative to the root).
    #[must_use]
    pub fn find(&self, rule: &str, path: &Path) -> Option<usize> {
        let wanted = normalise_rule(rule);
        self.exceptions.iter().position(|exception| {
            normalise_rule(&exception.rule) == wanted
                && globset::Glob::new(&exception.path)
                    .is_ok_and(|glob| glob.compile_matcher().is_match(path))
        })
    }
}

/// Canonical form of a rule identifier: rustc, Clippy and rustdoc lint names use underscores, while
/// command lines and Cargo accept hyphens. Other namespaces are left untouched.
#[must_use]
pub fn normalise_rule(rule: &str) -> String {
    let rule = rule.trim();
    match rule.split_once("::") {
        None => rule.replace('-', "_"),
        Some((tool @ ("clippy" | "rustdoc"), name)) => {
            format!("{tool}::{}", name.replace('-', "_"))
        }
        Some(_) => rule.to_owned(),
    }
}

/// Why a justification is unacceptable, or `None` when it is specific enough.
#[must_use]
pub fn justification_problem(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some("is empty");
    }
    let lowered = trimmed.to_lowercase();
    let bare = lowered.trim_matches(|c: char| !c.is_alphanumeric());
    if GENERIC.contains(&bare) {
        return Some("is generic; state why the rule is wrong here and why the exception is safe");
    }
    let has_placeholder = lowered
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| PLACEHOLDERS.contains(&word));
    if has_placeholder {
        return Some("contains a placeholder (TODO, TBD, FIXME, XXX)");
    }
    if trimmed.chars().count() < MIN_JUSTIFICATION_CHARS {
        return Some("is too short to explain the decision");
    }
    None
}
