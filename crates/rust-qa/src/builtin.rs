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

/// Every built-in identifier, for listing and documentation checks.
pub const ALL: [&str; 9] = [
    RECORD_INVALID,
    RECORD_STALE,
    RECORD_BUDGET,
    RECORD_RATCHET,
    RECORD_EXPIRED,
    SUPPRESSION_UNRECORDED,
    SUPPRESSION_ALLOW,
    SUMMARY_STALE,
    DOCS_DANGLING,
];
