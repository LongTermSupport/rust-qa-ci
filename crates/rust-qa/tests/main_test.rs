//! End-to-end: the `rust-qa` binary against small fixture workspaces.
//!
//! Fixture sources spell the allow attribute `LINTOFF(` and expand it at write time, so this file
//! never carries a literal suppression itself.
#![cfg(test)]

use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Output};

use rust_qa::date::Date;

const GOOD: &str =
    "The generated parser module is regenerated from the grammar and never edited by hand.";

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
    write(
        root,
        "src/lib.rs",
        "//! Fixture.\n\n/// Adds.\npub fn add(a: u8, b: u8) -> Option<u8> {\n    a.checked_add(b)\n}\n",
    );
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
        &format!(
            "[[exception]]\nrule = \"dead_code\"\npath = \"src/gen.rs\"\njustification = \"{GOOD}\"\n"
        ),
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
    assert!(
        value["defences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "clippy::unwrap_used")
    );
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
    write(
        dir.path(),
        "rules/r.yml",
        "id: undocumented-rule\nlanguage: rust\nseverity: error\nrule:\n  pattern: x\n",
    );
    let output = rust_qa(dir.path(), &["docs-check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("undocumented-rule"));
}

#[test]
fn docs_detector_blocks_the_gate_on_a_dangling_identifier() {
    let dir = fixture();
    write(dir.path(), "sgconfig.yml", "ruleDirs:\n  - rules\n");
    write(
        dir.path(),
        "rules/r.yml",
        "id: undocumented-rule\nlanguage: rust\nseverity: error\nrule:\n  pattern: x\n",
    );
    let output = rust_qa(dir.path(), &["run", "-t", "docs", "-t", "test"]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("undocumented-rule"));
    assert!(text.contains("runners were not started"));
}

#[test]
fn bundled_ast_grep_rules_are_listed_and_documented() {
    let dir = fixture();
    let text = stdout(&rust_qa(dir.path(), &["rules"]));
    assert!(text.contains("rqaci.ast.shell-c-command"), "{text}");
    for id in [
        "rqaci.ast.hardcoded-temp-path",
        "ast-grep::rqaci.ast.hardcoded-temp-path",
    ] {
        let output = rust_qa(dir.path(), &["rule-doc", id]);
        assert!(output.status.success(), "{id}");
        assert!(stdout(&output).starts_with("# rqaci.ast.hardcoded-temp-path\n"));
    }
}

/// A file violating a bundled rule, plus a project rule matching the same file.
fn ast_grep_fixture() -> tempfile::TempDir {
    let dir = fixture();
    write(
        dir.path(),
        "src/scratch.rs",
        "pub fn scratch() -> std::path::PathBuf {\n    std::path::PathBuf::from(\"/tmp/scratch\")\n}\n",
    );
    write(dir.path(), "sgconfig.yml", "ruleDirs:\n  - rules\n");
    write(
        dir.path(),
        "rules/r.yml",
        "id: no-scratch\nlanguage: rust\nseverity: error\nmessage: no scratch\nrule:\n  pattern: pub fn scratch() -> $T { $$$ }\n",
    );
    dir
}

#[test]
fn ast_grep_detector_reports_project_and_bundled_findings_in_one_run() {
    let dir = ast_grep_fixture();
    let output = rust_qa(dir.path(), &["run", "-t", "ast-grep"]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("error[no-scratch]"), "{text}");
    assert!(
        text.contains("error[rqaci.ast.hardcoded-temp-path]"),
        "{text}"
    );
}

#[test]
fn ast_grep_bundled_pack_opt_out_keeps_project_rules() {
    let dir = ast_grep_fixture();
    write(
        dir.path(),
        "qa/rust-qa.toml",
        "[ast_grep]\nbundled = false\n",
    );
    let output = rust_qa(dir.path(), &["run", "-t", "ast-grep"]);
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("error[no-scratch]"), "{text}");
    assert!(!text.contains("rqaci.ast."), "{text}");
    std::fs::remove_file(dir.path().join("sgconfig.yml")).unwrap();
    let output = rust_qa(dir.path(), &["run", "-t", "ast-grep"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("skipped"), "{text}");
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
    write(
        dir.path(),
        "src/extra.rs",
        "#[LINTOFF(dead_code)]\nfn unused() {}\n",
    );
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
    write(
        dir.path(),
        "src/extra.rs",
        "#[expect(dead_code, reason = \"generated\")]\nfn unused() {}\n",
    );
    write(dir.path(), "qa/rust-qa.toml", DECIDERS);
    write(
        dir.path(),
        "qa/record.toml",
        &signed_record(1, &[("dead_code", "src/extra.rs")]),
    );
    let output = rust_qa(dir.path(), &["run", "-t", "record", "-t", "suppression"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("Record budget: 1/10 exceptions (high water 1)"),
        "{text}"
    );
}

const DECIDERS: &str = "[record]\ndeciders = [\"alice\"]\n";

/// A record whose ratchet and entries are signed by `alice` today, due for review in 30 days.
fn signed_record(high_water: usize, entries: &[(&str, &str)]) -> String {
    let today = Date::today();
    let review = today.add_days(30);
    let mut text = format!(
        "[ratchet]\nhigh_water = {high_water}\ndecided_by = \"alice\"\ndecided_on = {today}\nreason = \"{GOOD}\"\n"
    );
    for (rule, path) in entries {
        let _ = write!(
            text,
            "\n[[exception]]\nrule = \"{rule}\"\npath = \"{path}\"\njustification = \"{GOOD}\"\n\
             decided_by = \"alice\"\nadded_by_model = \"human\"\ndecided_on = {today}\nreview_by = {review}\n"
        );
    }
    text
}

#[test]
fn record_detector_lowers_the_ratchet_locally_and_fails_in_ci() {
    let dir = fixture();
    write(
        dir.path(),
        "src/extra.rs",
        "#[expect(dead_code, reason = \"generated\")]\nfn unused() {}\n",
    );
    write(dir.path(), "qa/rust-qa.toml", DECIDERS);
    write(
        dir.path(),
        "qa/record.toml",
        &signed_record(3, &[("dead_code", "src/extra.rs")]),
    );
    let ci = rust_qa(dir.path(), &["run", "--ci", "-t", "record"]);
    assert_eq!(ci.status.code(), Some(1), "{}", stdout(&ci));
    assert!(stdout(&ci).contains("error[rqaci.record.ratchet]"));
    let local = rust_qa(dir.path(), &["run", "-t", "record"]);
    assert!(local.status.success(), "{}", stdout(&local));
    assert!(stdout(&local).contains("lowered [ratchet] high_water to 1"));
    let text = std::fs::read_to_string(dir.path().join("qa/record.toml")).unwrap();
    assert!(text.contains("high_water = 1\n"), "{text}");
}

#[test]
fn a_bloated_record_fails_every_control() {
    let dir = fixture();
    write(
        dir.path(),
        "qa/rust-qa.toml",
        "[record]\nmax_total = 3\nmax_per_rule = 2\ndeciders = [\"alice\"]\n",
    );
    let mut text = signed_record(
        3,
        &[
            ("dead_code", "src/a.rs"),
            ("dead_code", "src/b.rs"),
            ("dead_code", "src/c.rs"),
        ],
    );
    let glob = concat!("src", "/", "*.rs");
    let _ = write!(
        text,
        "\n[[exception]]\nrule = \"unused\"\npath = \"{glob}\"\njustification = \"{GOOD}\"\n\
         decided_by = \"someone\"\nadded_by_model = \"mystery-model\"\ndecided_on = 2020-01-01\nreview_by = 2020-02-01\n"
    );
    write(dir.path(), "qa/record.toml", &text);
    let output = rust_qa(dir.path(), &["record", "check"]);
    let out = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{out}");
    for id in [
        "error[rqaci.record.budget]",
        "error[rqaci.record.ratchet]",
        "error[rqaci.record.expired]",
        "error[rqaci.record.invalid]",
    ] {
        assert!(out.contains(id), "{id} missing:\n{out}");
    }
    for fragment in ["4 exceptions exceed", "mystery-model", "is a glob", "grew"] {
        assert!(out.contains(fragment), "{fragment} missing:\n{out}");
    }
}

#[test]
fn record_prune_removes_stale_entries_and_lowers_the_ratchet() {
    let dir = fixture();
    write(
        dir.path(),
        "src/extra.rs",
        "#[expect(dead_code, reason = \"generated\")]\nfn unused() {}\n",
    );
    write(dir.path(), "qa/rust-qa.toml", DECIDERS);
    write(
        dir.path(),
        "qa/record.toml",
        &signed_record(
            2,
            &[("unused", "src/gone.rs"), ("dead_code", "src/extra.rs")],
        ),
    );
    let dry = rust_qa(dir.path(), &["record", "prune", "--dry-run"]);
    assert!(stdout(&dry).contains("stale: exception #1 `unused`"));
    let before = std::fs::read_to_string(dir.path().join("qa/record.toml")).unwrap();
    assert!(before.contains("src/gone.rs"));
    let pruned = rust_qa(dir.path(), &["record", "prune"]);
    assert!(pruned.status.success(), "{}", stdout(&pruned));
    let after = std::fs::read_to_string(dir.path().join("qa/record.toml")).unwrap();
    assert!(!after.contains("src/gone.rs"), "{after}");
    assert!(after.contains("high_water = 1\n"), "{after}");
    let check = rust_qa(dir.path(), &["run", "-t", "record", "-t", "suppression"]);
    assert!(check.status.success(), "{}", stdout(&check));
}

#[test]
fn record_explain_shows_provenance_deadline_and_covered_sites() {
    let dir = fixture();
    write(
        dir.path(),
        "src/extra.rs",
        "#[expect(dead_code, reason = \"generated\")]\nfn unused() {}\n",
    );
    write(dir.path(), "qa/rust-qa.toml", DECIDERS);
    write(
        dir.path(),
        "qa/record.toml",
        &signed_record(1, &[("dead_code", "src/extra.rs")]),
    );
    let output = rust_qa(dir.path(), &["record", "explain", "dead_code"]);
    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("tier human"), "{text}");
    assert!(text.contains("(30 day(s) left)"), "{text}");
    assert!(text.contains("covers: src/extra.rs:1"), "{text}");
    let by_number = stdout(&rust_qa(dir.path(), &["record", "explain", "1"]));
    assert!(
        by_number.contains("exception #1 `dead_code`"),
        "{by_number}"
    );
    let missing = rust_qa(dir.path(), &["record", "explain", "unused"]);
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn rules_print_the_budget_status() {
    let dir = fixture();
    let text = stdout(&rust_qa(dir.path(), &["rules"]));
    assert!(text.contains("Record budget: 0/10 exceptions"), "{text}");
    for id in [
        "rqaci.record.budget",
        "rqaci.record.ratchet",
        "rqaci.record.expired",
        "rqaci.suppression.allow",
    ] {
        assert!(text.contains(id), "{id} not listed:\n{text}");
    }
    let json: serde_json::Value =
        serde_json::from_slice(&rust_qa(dir.path(), &["rules", "--json"]).stdout).unwrap();
    assert_eq!(json["budget"]["max_total"], 10);
}

#[test]
fn summary_writes_a_region_and_check_mode_detects_staleness() {
    let dir = fixture();
    write(
        dir.path(),
        "qa/rust-qa.toml",
        "[summary]\nfile = \"AGENTS.md\"\n",
    );
    write(dir.path(), "AGENTS.md", "# Agents\n");
    let check = rust_qa(dir.path(), &["summary", "--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(stdout(&check).contains("rqaci.summary.stale"));
    assert!(rust_qa(dir.path(), &["summary"]).status.success());
    let text = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(text.starts_with("# Agents\n\n<!-- BEGIN rust-qa-ci"));
    assert!(text.contains("`clippy::unwrap_used`"));
    assert!(
        rust_qa(dir.path(), &["summary", "--check"])
            .status
            .success()
    );
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
