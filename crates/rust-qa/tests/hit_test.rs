//! Findings reported by detectors that apply the project record themselves: how they print, and
//! which record exceptions cover them.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::builtin::RECORD_STALE;
use rust_qa::hit::{Hit, apply, unused};
use rust_qa::record::{Covers, Exception, Record};

fn hit(rule: &str, path: &str) -> Hit {
    Hit {
        rule: rule.to_owned(),
        path: PathBuf::from(path),
        line: 3,
        column: 7,
        message: "the value is cut short".to_owned(),
        note: Some("quote the value".to_owned()),
        rendered: None,
        blocking: true,
    }
}

fn exception(rule: &str, path: &str, covers: Covers) -> Exception {
    Exception {
        rule: rule.to_owned(),
        path: path.to_owned(),
        justification: "The generated table is regenerated from its schema and never edited."
            .to_owned(),
        covers,
        ..Exception::default()
    }
}

fn record(exceptions: Vec<Exception>) -> Record {
    Record {
        ratchet: None,
        exceptions,
    }
}

#[test]
fn a_finding_prints_its_rule_place_note_and_how_to_resolve_the_rule() {
    let text = hit("my-rule", "src/a.rs").to_string();
    assert_eq!(
        text,
        "error[my-rule]: the value is cut short\n  --> src/a.rs:3:7\n  = note: quote the value\n  = doc: rust-qa rule-doc my-rule"
    );
}

#[test]
fn a_non_blocking_finding_prints_as_a_warning() {
    let mut finding = hit("my-rule", "src/a.rs");
    finding.blocking = false;
    finding.note = None;
    assert!(finding.to_string().starts_with("warning[my-rule]: "));
}

#[test]
fn a_finding_rendered_by_its_tool_keeps_that_rendering() {
    let mut finding = hit("my_lint", "src/a.rs");
    finding.rendered = Some("error: compiler text\n --> src/a.rs:3:7\n".to_owned());
    assert_eq!(
        finding.to_string(),
        "error: compiler text\n --> src/a.rs:3:7\n  = doc: rust-qa rule-doc my_lint"
    );
}

#[test]
fn findings_exceptions_cover_findings_and_suppression_exceptions_do_not() {
    let record = record(vec![
        exception("my-rule", "src/gen/*.rs", Covers::Suppressions),
        exception("my-rule", "src/gen/table.rs", Covers::Findings),
    ]);
    let applied = apply(
        vec![
            hit("my-rule", "src/gen/table.rs"),
            hit("my-rule", "src/gen/other.rs"),
            hit("other-rule", "src/gen/table.rs"),
        ],
        &record,
    );
    let kept: Vec<(&str, &Path)> = applied
        .kept
        .iter()
        .map(|h| (h.rule.as_str(), h.path.as_path()))
        .collect();
    assert_eq!(
        kept,
        vec![
            ("my-rule", Path::new("src/gen/other.rs")),
            ("other-rule", Path::new("src/gen/table.rs"))
        ]
    );
    assert_eq!(applied.excepted, 1);
    assert_eq!(applied.used, vec![1]);
}

#[test]
fn a_lint_name_matches_its_identifier_in_any_case_with_hyphens() {
    let record = record(vec![exception(
        "MY-TYPE-RULE",
        "src/a.rs",
        Covers::Findings,
    )]);
    let applied = apply(vec![hit("my_type_rule", "src/a.rs")], &record);
    assert_eq!(applied.kept, Vec::<Hit>::new());
}

#[test]
fn non_blocking_findings_are_kept_but_need_no_exception() {
    let mut warning = hit("my-rule", "src/a.rs");
    warning.blocking = false;
    let applied = apply(vec![warning], &record(Vec::new()));
    assert_eq!(applied.kept.len(), 1);
    assert_eq!(applied.blocking(), 0);
}

#[test]
fn an_unused_findings_exception_for_a_hosted_rule_is_stale() {
    let record = record(vec![
        exception("my-rule", "src/a.rs", Covers::Findings),
        exception("my-rule", "src/b.rs", Covers::Findings),
        exception("elsewhere-rule", "src/c.rs", Covers::Findings),
        exception("my-rule", "src/d.rs", Covers::Suppressions),
    ]);
    let stale = unused(
        &record,
        &[0],
        &["my-rule".to_owned()],
        Path::new("qa/record.toml"),
    );
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].rule, RECORD_STALE);
    assert!(
        stale[0].message.contains("exception #2"),
        "{}",
        stale[0].message
    );
}
