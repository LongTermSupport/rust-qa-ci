//! Cargo JSON diagnostics: parsing and filtering to a file subset.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::diagnostics::{Diagnostic, dedupe, in_scope, parse_cargo_messages};

fn message(code: &str, level: &str, file: &str) -> String {
    serde_json::json!({
        "reason": "compiler-message",
        "message": {
            "rendered": format!("{level}: thing\n --> {file}:1:1\n"),
            "level": level,
            "code": {"code": code, "explanation": null},
            "spans": [
                {"file_name": file, "is_primary": true, "line_start": 1},
                {"file_name": "other.rs", "is_primary": false, "line_start": 2}
            ]
        }
    })
    .to_string()
}

#[test]
fn compiler_messages_are_parsed_and_other_lines_ignored() {
    let stream = format!(
        "{}\n{{\"reason\":\"compiler-artifact\"}}\nnot json\n{}\n",
        message("clippy::unwrap_used", "error", "crates/a/src/lib.rs"),
        message("dead_code", "warning", "crates/b/src/lib.rs")
    );
    let diags = parse_cargo_messages(&stream);
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[0].code.as_deref(), Some("clippy::unwrap_used"));
    assert!(diags[0].is_error());
    assert_eq!(diags[0].files, vec![PathBuf::from("crates/a/src/lib.rs")]);
    assert!(!diags[1].is_error());
}

#[test]
fn scope_matches_primary_span_files_by_path_prefix() {
    let diag = Diagnostic {
        code: Some("x".to_owned()),
        level: "error".to_owned(),
        rendered: String::new(),
        files: vec![PathBuf::from("crates/a/src/lib.rs")],
    };
    let root = Path::new("/ws");
    assert!(in_scope(
        &diag,
        root,
        &[PathBuf::from("crates/a/src/lib.rs")]
    ));
    assert!(in_scope(&diag, root, &[PathBuf::from("crates/a")]));
    assert!(!in_scope(&diag, root, &[PathBuf::from("crates/b")]));
    assert!(!in_scope(
        &diag,
        root,
        &[PathBuf::from("crates/a/src/main.rs")]
    ));
}

#[test]
fn absolute_span_paths_are_made_relative_to_the_root() {
    let diag = Diagnostic {
        code: None,
        level: "error".to_owned(),
        rendered: String::new(),
        files: vec![PathBuf::from("/ws/crates/a/src/lib.rs")],
    };
    assert!(in_scope(
        &diag,
        Path::new("/ws"),
        &[PathBuf::from("crates/a")]
    ));
}

#[test]
fn identical_diagnostics_from_several_targets_are_reported_once() {
    let one = message("example_rule", "error", "src/lib.rs");
    let other = message("example_rule", "error", "src/main.rs");
    let stream = format!("{one}\n{one}\n{other}\n");
    let diags = dedupe(parse_cargo_messages(&stream));
    assert_eq!(diags.len(), 2);
    assert_eq!(diags[1].files, vec![PathBuf::from("src/main.rs")]);
}

#[test]
fn diagnostics_without_spans_are_never_in_scope() {
    let diag = Diagnostic {
        code: None,
        level: "error".to_owned(),
        rendered: "error: aborting".to_owned(),
        files: Vec::new(),
    };
    assert!(!in_scope(
        &diag,
        Path::new("/ws"),
        &[PathBuf::from("crates/a")]
    ));
}
