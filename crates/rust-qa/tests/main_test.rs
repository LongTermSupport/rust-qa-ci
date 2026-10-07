//! End-to-end: the `rust-qa` binary against small fixture workspaces.
//!
//! Fixture sources spell the allow attribute `LINTOFF(` and expand it at write time, so this file
//! never carries a literal suppression itself.
#![cfg(test)]

use std::path::Path;
use std::process::{Command, Output};

const GOOD: &str = "The generated parser module is regenerated from the grammar and never edited by hand.";

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text.replace("LINTOFF(", "allow(")).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n\
         [lints.clippy]\nunwrap_used = \"deny\"\n",
    );
    write(root, "src/lib.rs", "//! Fixture.\n\n/// Adds.\npub fn add(a: u8, b: u8) -> Option<u8> {\n    a.checked_add(b)\n}\n");
    dir
}

fn rust_qa(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust-qa"))
        .arg("--root")
        .arg(root)
        .args(args)
        .env_remove("CI")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn rules_lists_defences_derived_from_configuration_and_the_record() {
    let dir = fixture();
    write(
        dir.path(),
        "qa/record.toml",
        &format!("[[exception]]\nrule = \"dead_code\"\npath = \"src/gen.rs\"\njustification = \"{GOOD}\"\n"),
    );
    let output = rust_qa(dir.path(), &["rules"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("clippy::unwrap_used"));
    assert!(text.contains("rqaci.suppression.unrecorded"));
    assert!(text.contains("Project record"));
    assert!(text.contains("src/gen.rs"));
}

#[test]
fn rules_json_is_machine_readable() {
    let dir = fixture();
    let output = rust_qa(dir.path(), &["rules", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["defences"].as_array().unwrap().iter().any(|d| d["id"] == "clippy::unwrap_used"));
    assert_eq!(value["record"].as_array().unwrap().len(), 0);
}

#[test]
fn rule_doc_prints_bundled_docs_and_fails_on_dangling_ids() {
    let dir = fixture();
    let output = rust_qa(dir.path(), &["rule-doc", "rqaci.record.invalid"]);
    assert!(output.status.success());
    assert!(stdout(&output).starts_with("# rqaci.record.invalid"));
    let missing = rust_qa(dir.path(), &["rule-doc", "no_such_rule_anywhere"]);
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn docs_check_passes_when_every_listed_id_resolves() {
    let dir = fixture();
    let output = rust_qa(dir.path(), &["docs-check"]);
    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn docs_check_fails_on_a_dangling_project_rule() {
    let dir = fixture();
    write(dir.path(), "sgconfig.yml", "ruleDirs:\n  - rules\n");
    write(dir.path(), "rules/r.yml", "id: undocumented-rule\nlanguage: rust\nseverity: error\nrule:\n  pattern: x\n");
    let output = rust_qa(dir.path(), &["docs-check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("undocumented-rule"));
}

#[test]
fn record_check_rejects_generic_justifications() {
    let dir = fixture();
    write(
        dir.path(),
        "qa/record.toml",
        "[[exception]]\nrule = \"dead_code\"\npath = \"src/lib.rs\"\njustification = \"legacy\"\n",
    );
    let output = rust_qa(dir.path(), &["record", "check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("error[rqaci.record.invalid]"));
}

#[test]
fn unrecorded_suppression_fails_detectors_and_runners_never_start() {
    let dir = fixture();
    write(dir.path(), "src/extra.rs", "#[LINTOFF(dead_code)]\nfn unused() {}\n");
    let output = rust_qa(dir.path(), &["run", "-t", "suppression", "-t", "test"]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("error[rqaci.suppression.unrecorded]"));
    assert!(text.contains("src/extra.rs:1"));
    assert!(text.contains("Defence Before Fix"));
    assert!(text.contains("runners were not started"));
    assert!(!text.contains("==> [runners]"));
}

#[test]
fn recorded_suppression_passes() {
    let dir = fixture();
    write(dir.path(), "src/extra.rs", "#[expect(dead_code, reason = \"generated\")]\nfn unused() {}\n");
    write(
        dir.path(),
        "qa/record.toml",
        &format!("[[exception]]\nrule = \"dead_code\"\npath = \"src/extra.rs\"\njustification = \"{GOOD}\"\n"),
    );
    let output = rust_qa(dir.path(), &["run", "-t", "record", "-t", "suppression"]);
    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn summary_writes_a_region_and_check_mode_detects_staleness() {
    let dir = fixture();
    write(dir.path(), "qa/rust-qa.toml", "[summary]\nfile = \"AGENTS.md\"\n");
    write(dir.path(), "AGENTS.md", "# Agents\n");
    let check = rust_qa(dir.path(), &["summary", "--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(stdout(&check).contains("rqaci.summary.stale"));
    assert!(rust_qa(dir.path(), &["summary"]).status.success());
    let text = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(text.starts_with("# Agents\n\n<!-- BEGIN rust-qa-ci"));
    assert!(text.contains("`clippy::unwrap_used`"));
    assert!(rust_qa(dir.path(), &["summary", "--check"]).status.success());
}

#[test]
fn unknown_tool_is_a_usage_error() {
    let dir = fixture();
    let output = rust_qa(dir.path(), &["run", "-t", "clipy"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn tools_lists_the_catalogue_with_phases() {
    let dir = fixture();
    let text = stdout(&rust_qa(dir.path(), &["tools"]));
    assert!(text.contains("clippy"));
    assert!(text.contains("detectors"));
    assert!(text.contains("runners"));
}
