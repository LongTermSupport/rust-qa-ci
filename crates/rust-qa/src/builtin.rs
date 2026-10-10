//! Identifiers of the defences implemented inside `rust-qa` itself.
//!
//! Identifiers are chosen by hand and never derived from paths or type names, so they stay stable
//! across refactors. Each one has a bundled document under `docs/rules/<id>.md`.

/// A project-record exception is malformed or its justification is empty or generic.
pub const RECORD_INVALID: &str = "rqaci.record.invalid";
/// A project-record exception matches no suppression in the workspace.
pub const RECORD_STALE: &str = "rqaci.record.stale";
/// The project record exceeds a configured budget (total, per rule, policy-level or per trust tier).
pub const RECORD_BUDGET: &str = "rqaci.record.budget";
/// The project record grew past its signed ceiling, or the ceiling is unsigned or not lowered.
pub const RECORD_RATCHET: &str = "rqaci.record.ratchet";
/// A project-record exception is past its review date.
pub const RECORD_EXPIRED: &str = "rqaci.record.expired";
/// A suppression route is used without a matching project-record exception.
pub const SUPPRESSION_UNRECORDED: &str = "rqaci.suppression.unrecorded";
/// A lint is silenced with `#[allow]`, which never reports when it stops being needed.
pub const SUPPRESSION_ALLOW: &str = "rqaci.suppression.allow";
/// The generated agent summary region is out of date.
pub const SUMMARY_STALE: &str = "rqaci.summary.stale";

/// A listed defence identifier has no resolvable documentation.
pub const DOCS_DANGLING: &str = "rqaci.docs.dangling";
/// A page in the project's documentation directory documents no listed identifier.
pub const DOCS_ORPHAN: &str = "rqaci.docs.orphan";
/// A project ast-grep rule has no test case with both valid and invalid code.
pub const RULES_UNTESTED: &str = "rqaci.rules.untested";

/// Every built-in identifier, for listing and documentation checks.
pub const ALL: [&str; 11] = [
    RECORD_INVALID,
    RECORD_STALE,
    RECORD_BUDGET,
    RECORD_RATCHET,
    RECORD_EXPIRED,
    SUPPRESSION_UNRECORDED,
    SUPPRESSION_ALLOW,
    SUMMARY_STALE,
    DOCS_DANGLING,
    DOCS_ORPHAN,
    RULES_UNTESTED,
];

/// One line saying what a built-in identifier guards, for the listing and the agent summary.
#[must_use]
pub fn describe(id: &str) -> &'static str {
    match id {
        RECORD_INVALID => "every project-record exception is well formed, attributed and justified",
        RECORD_STALE => "every project-record exception still covers something",
        RECORD_BUDGET => "the project record stays within its budgets",
        RECORD_RATCHET => "the project record grows only by a signed decision",
        RECORD_EXPIRED => "no project-record exception is past its review date",
        SUPPRESSION_UNRECORDED => "every suppression has an exception in the project record",
        SUPPRESSION_ALLOW => "lints are silenced with `expect`, never `allow`",
        SUMMARY_STALE => "the generated agent summary is current",
        DOCS_DANGLING => "every listed identifier resolves to documentation",
        DOCS_ORPHAN => "every documentation page belongs to a listed identifier",
        RULES_UNTESTED => "every project ast-grep rule has valid and invalid test cases",
        _ => "",
    }
}
