//! Identifiers of the defences implemented inside `rust-qa` itself.
//!
//! Identifiers are chosen by hand and never derived from paths or type names, so they stay stable
//! across refactors. Each one has a bundled document under `docs/rules/<id>.md`.

/// A project-record exception is malformed or its justification is empty or generic.
pub const RECORD_INVALID: &str = "rqaci.record.invalid";
/// A project-record exception matches no suppression in the workspace.
pub const RECORD_STALE: &str = "rqaci.record.stale";
/// A suppression route is used without a matching project-record exception.
pub const SUPPRESSION_UNRECORDED: &str = "rqaci.suppression.unrecorded";
/// The generated agent summary region is out of date.
pub const SUMMARY_STALE: &str = "rqaci.summary.stale";

/// Every built-in identifier, for listing and documentation checks.
pub const ALL: [&str; 4] = [
    RECORD_INVALID,
    RECORD_STALE,
    SUPPRESSION_UNRECORDED,
    SUMMARY_STALE,
];
