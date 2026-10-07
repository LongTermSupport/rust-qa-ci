//! Finding output format: identifier printed unaltered, location, doc route.
#![cfg(test)]

use std::path::PathBuf;

use rust_qa::finding::Finding;

#[test]
fn display_prints_identifier_location_and_doc_route() {
    let finding = Finding {
        rule: "rqaci.example",
        path: PathBuf::from("src/lib.rs"),
        line: 7,
        message: "bad thing".to_owned(),
    };
    assert_eq!(
        finding.to_string(),
        "error[rqaci.example]: bad thing\n  --> src/lib.rs:7\n  = doc: rust-qa rule-doc rqaci.example"
    );
}

#[test]
fn display_omits_line_for_whole_file_findings() {
    let finding = Finding {
        rule: "rqaci.example",
        path: PathBuf::from("qa/record.toml"),
        line: 0,
        message: "m".to_owned(),
    };
    assert!(finding.to_string().contains("  --> qa/record.toml\n"));
}
