//! Agent-context summary of active defences, generated into a delimited region of a file that agents
//! already load. Text outside the markers is never touched.

use std::fmt::Write as _;

use crate::defences::Defence;
use crate::policy::{RecordPolicy, Status};
use crate::record::Record;

/// Opening marker of the generated region.
pub const BEGIN: &str =
    "<!-- BEGIN rust-qa-ci generated defences: run `rust-qa summary` to refresh; do not edit -->";
/// Closing marker of the generated region.
pub const END: &str = "<!-- END rust-qa-ci generated defences -->";

/// Renders the region, markers included, ending with a newline.
#[must_use]
pub fn render(defences: &[Defence], record: &Record, policy: &RecordPolicy) -> String {
    let mut out = String::new();
    out.push_str(BEGIN);
    out.push_str(
        "\n\nActive defences. Failures print these ids unaltered; resolve any id offline with ",
    );
    out.push_str("`rust-qa rule-doc <id>`. Exceptions only via the project record, with a justification.\n\n");
    for defence in defences {
        let _ = writeln!(
            out,
            "- `{}` ({}, {}): `rust-qa rule-doc {}`",
            defence.id,
            defence.engine.name(),
            defence.level,
            defence.id
        );
    }
    out.push_str(
        "\nProject-record exceptions (prefer an in-source `#[expect(lint, reason = \"...\")]`, ",
    );
    out.push_str("which fails once the lint stops firing):\n\n");
    if record.exceptions.is_empty() {
        out.push_str("- none\n");
    }
    for exception in &record.exceptions {
        let review_by = exception.review_by.map_or_else(
            || "no review date".to_owned(),
            |date| format!("review by {date}"),
        );
        let model = exception.added_by_model.as_deref().unwrap_or("unknown");
        let _ = writeln!(
            out,
            "- `{}` at `{}` ({review_by}, added by {model})",
            exception.rule, exception.path
        );
    }
    out.push_str("\nRecord budget (`rust-qa record list`):\n\n");
    for line in Status::of(record, policy).to_string().lines() {
        let _ = writeln!(out, "- {line}");
    }
    out.push('\n');
    out.push_str(END);
    out.push('\n');
    out
}

/// Replaces the region in `existing`, or appends it after a blank line when absent.
#[must_use]
pub fn splice(existing: &str, region: &str) -> String {
    if let (Some(start), Some(end)) = (existing.find(BEGIN), existing.find(END))
        && start < end
    {
        let after = existing.get(end + END.len()..).unwrap_or("");
        let after = after.strip_prefix('\n').unwrap_or(after);
        return format!("{}{region}{after}", existing.get(..start).unwrap_or(""));
    }
    let mut out = existing.trim_end().to_owned();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(region);
    out
}
