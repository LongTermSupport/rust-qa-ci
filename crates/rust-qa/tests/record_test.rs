//! Project record: loading, justification validation and exception matching.
#![cfg(test)]

use std::path::Path;

use rust_qa::builtin::RECORD_INVALID;
use rust_qa::record::{Exception, Record, justification_problem};

const GOOD: &str =
    "Tests assert on panics deliberately; a panic is the failure signal in test code.";

fn exception(rule: &str, path: &str, justification: &str) -> Exception {
    Exception {
        rule: rule.to_owned(),
        path: path.to_owned(),
        justification: justification.to_owned(),
        decided_by: Some("owner".to_owned()),
    }
}

fn record(exceptions: Vec<Exception>) -> Record {
    Record { exceptions }
}

#[test]
fn missing_record_file_is_an_empty_record() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = Record::load(&dir.path().join("qa/record.toml")).unwrap();
    assert!(loaded.exceptions.is_empty());
}

#[test]
fn record_file_parses_exceptions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("record.toml");
    std::fs::write(
        &path,
        format!(
            "[[exception]]\nrule = \"clippy::unwrap_used\"\npath = \"tests/a.rs\"\njustification = \"{GOOD}\"\n"
        ),
    )
    .unwrap();
    let loaded = Record::load(&path).unwrap();
    assert_eq!(loaded.exceptions.len(), 1);
    assert_eq!(loaded.exceptions[0].rule, "clippy::unwrap_used");
    assert!(loaded.exceptions[0].decided_by.is_none());
}

#[test]
fn record_file_rejects_unknown_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("record.toml");
    std::fs::write(
        &path,
        "[[exception]]\nrule = \"x\"\npath = \"y\"\njustification = \"z\"\nreson = \"typo\"\n",
    )
    .unwrap();
    assert!(Record::load(&path).is_err());
}

#[test]
fn specific_justification_is_accepted() {
    assert_eq!(justification_problem(GOOD), None);
    let findings = record(vec![exception("dead_code", "src/a.rs", GOOD)])
        .validate(Path::new("qa/record.toml"));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn empty_justification_is_rejected() {
    assert!(justification_problem("   ").is_some());
}

#[test]
fn generic_justifications_are_rejected() {
    for text in [
        "TODO",
        "legacy",
        "Needed for now.",
        "temporary",
        "n/a",
        "fix later",
        "WIP",
        "false positive",
    ] {
        assert!(justification_problem(text).is_some(), "{text} was accepted");
    }
}

#[test]
fn placeholder_words_inside_longer_text_are_rejected() {
    assert!(justification_problem("TODO: explain properly why this lint is wrong here").is_some());
    assert!(justification_problem("FIXME once the upstream crate exposes a typed error").is_some());
}

#[test]
fn too_short_justification_is_rejected() {
    assert!(justification_problem("it is fine").is_some());
}

#[test]
fn validate_reports_each_bad_exception_with_its_rule() {
    let findings = record(vec![
        exception("dead_code", "src/a.rs", "legacy"),
        exception("", "src/a.rs", GOOD),
        exception("dead_code", "", GOOD),
        exception("dead_code", "src/[a.rs", GOOD),
        exception("dead_code", "src/a.rs", GOOD),
    ])
    .validate(Path::new("qa/record.toml"));
    assert_eq!(findings.len(), 4, "{findings:?}");
    assert!(findings.iter().all(|f| f.rule == RECORD_INVALID));
    assert!(findings[0].message.contains("dead_code"));
}

#[test]
fn empty_decided_by_is_rejected() {
    let mut bad = exception("dead_code", "src/a.rs", GOOD);
    bad.decided_by = Some(" ".to_owned());
    assert_eq!(record(vec![bad]).validate(Path::new("r.toml")).len(), 1);
}

#[test]
fn find_matches_rule_and_path_glob() {
    let rec = record(vec![
        exception("clippy::unwrap_used", "tests/**", GOOD),
        exception("dead_code", "src/a.rs", GOOD),
    ]);
    assert_eq!(
        rec.find("clippy::unwrap_used", Path::new("tests/x/y.rs")),
        Some(0)
    );
    assert_eq!(rec.find("dead_code", Path::new("src/a.rs")), Some(1));
    assert_eq!(rec.find("dead_code", Path::new("src/b.rs")), None);
    assert_eq!(
        rec.find("clippy::expect_used", Path::new("tests/x.rs")),
        None
    );
}

#[test]
fn find_normalises_hyphenated_lint_names() {
    let rec = record(vec![exception("clippy::unwrap_used", "src/a.rs", GOOD)]);
    assert_eq!(
        rec.find("clippy::unwrap-used", Path::new("src/a.rs")),
        Some(0)
    );
}
