//! Agent summary: one line per defence and record exception, in a delimited generated region.
#![cfg(test)]

use rust_qa::defences::{Defence, Engine};
use rust_qa::record::{Exception, Record};
use rust_qa::summary::{BEGIN, END, render, splice};

fn defences() -> Vec<Defence> {
    vec![
        Defence {
            id: "clippy::unwrap_used".to_owned(),
            engine: Engine::Clippy,
            level: "deny".to_owned(),
            origin: "Cargo.toml:9".to_owned(),
        },
        Defence {
            id: "example_type_rule".to_owned(),
            engine: Engine::Dylint,
            level: "deny".to_owned(),
            origin: "dylint library x".to_owned(),
        },
    ]
}

fn record() -> Record {
    Record {
        exceptions: vec![Exception {
            rule: "clippy::unwrap_used".to_owned(),
            path: "tests/a.rs".to_owned(),
            justification: "j".to_owned(),
            decided_by: None,
        }],
    }
}

#[test]
fn render_is_delimited_with_one_line_per_defence_and_doc_route() {
    let region = render(&defences(), &record());
    assert!(region.starts_with(BEGIN));
    assert!(region.trim_end().ends_with(END));
    assert!(region.contains("- `clippy::unwrap_used` (clippy, deny): `rust-qa rule-doc clippy::unwrap_used`\n"));
    assert!(region.contains("- `example_type_rule` (dylint, deny): `rust-qa rule-doc example_type_rule`\n"));
    assert!(region.contains("- `clippy::unwrap_used` at `tests/a.rs`"));
}

#[test]
fn splice_appends_a_region_when_absent() {
    let region = render(&defences(), &record());
    let out = splice("---\npaths: [\"**/*.rs\"]\n---\n\nIntro.\n", &region);
    assert!(out.starts_with("---\npaths"));
    assert!(out.contains("Intro.\n\n<!-- BEGIN"));
}

#[test]
fn splice_replaces_only_the_region_and_is_idempotent() {
    let region = render(&defences(), &record());
    let original = format!("before\n\n{BEGIN}\nold content\n{END}\n\nafter\n");
    let once = splice(&original, &region);
    assert!(once.starts_with("before\n\n"));
    assert!(once.ends_with("\n\nafter\n"));
    assert!(!once.contains("old content"));
    assert_eq!(splice(&once, &region), once);
}
