//! Agent summary: one line per defence and record exception, in a delimited generated region.
#![cfg(test)]

use rust_qa::date::Date;
use rust_qa::defences::{Defence, Engine};
use rust_qa::policy::RecordPolicy;
use rust_qa::record::{Covers, Exception, Record};
use rust_qa::summary::{BEGIN, END, render, splice};

fn defences() -> Vec<Defence> {
    vec![
        Defence {
            id: "clippy::unwrap_used".to_owned(),
            engine: Engine::Clippy,
            level: "deny".to_owned(),
            origin: "Cargo.toml:9".to_owned(),
            description: String::new(),
        },
        Defence {
            id: "example_type_rule".to_owned(),
            engine: Engine::Dylint,
            level: "deny".to_owned(),
            origin: "dylint library x".to_owned(),
            description: "a value of a secret type is printed".to_owned(),
        },
    ]
}

fn record() -> Record {
    Record {
        ratchet: None,
        exceptions: vec![Exception {
            rule: "clippy::unwrap_used".to_owned(),
            path: "tests/a.rs".to_owned(),
            justification: "j".to_owned(),
            added_by_model: Some("human".to_owned()),
            review_by: Date::parse("2027-01-31"),
            ..Exception::default()
        }],
    }
}

#[test]
fn render_is_delimited_with_one_line_per_defence_and_doc_route() {
    let region = render(&defences(), &record(), &RecordPolicy::default());
    assert!(region.starts_with(BEGIN));
    assert!(region.trim_end().ends_with(END));
    assert!(region.contains(
        "- `clippy::unwrap_used` (clippy, deny): `rust-qa rule-doc clippy::unwrap_used`\n"
    ));
    assert!(
        region.contains(
            "- `example_type_rule` (dylint, deny): a value of a secret type is printed. `rust-qa rule-doc example_type_rule`\n"
        )
    );
    assert!(region.contains(
        "- `clippy::unwrap_used` at `tests/a.rs` (review by 2027-01-31, added by human)\n"
    ));
}

#[test]
fn render_shows_the_budget_status_and_prefers_expect() {
    let region = render(&defences(), &record(), &RecordPolicy::default());
    assert!(region.contains("#[expect(lint, reason"), "{region}");
    assert!(
        region.contains("- Record budget: 1/10 exceptions (no ratchet)"),
        "{region}"
    );
    assert!(region.contains("- By trust tier: human 1"), "{region}");
    assert!(
        region.contains("- soonest review: 2027-01-31 `clippy::unwrap_used` at `tests/a.rs`"),
        "{region}"
    );
}

#[test]
fn splice_appends_a_region_when_absent() {
    let region = render(&defences(), &record(), &RecordPolicy::default());
    let out = splice("---\npaths: [\"**/*.rs\"]\n---\n\nIntro.\n", &region);
    assert!(out.starts_with("---\npaths"));
    assert!(out.contains("Intro.\n\n<!-- BEGIN"));
}

#[test]
fn splice_replaces_only_the_region_and_is_idempotent() {
    let region = render(&defences(), &record(), &RecordPolicy::default());
    let original = format!("before\n\n{BEGIN}\nold content\n{END}\n\nafter\n");
    let once = splice(&original, &region);
    assert!(once.starts_with("before\n\n"));
    assert!(once.ends_with("\n\nafter\n"));
    assert!(!once.contains("old content"));
    assert_eq!(splice(&once, &region), once);
}

#[test]
fn a_findings_exception_says_what_it_covers() {
    let mut record = record();
    record.exceptions[0].covers = Covers::Findings;
    let region = render(&defences(), &record, &RecordPolicy::default());
    assert!(
        region.contains("(review by 2027-01-31, added by human, covers findings)\n"),
        "{region}"
    );
}
