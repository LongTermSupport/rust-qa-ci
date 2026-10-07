//! Configuration cascade: built-in defaults, then the project file.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::config::{Config, DEFAULT_PATH, MutantsMode};
use rust_qa::policy::Tier;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn missing_config_file_yields_documented_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::load(dir.path(), None).unwrap();
    assert_eq!(config.record.path, PathBuf::from("qa/record.toml"));
    assert_eq!(config.record.max_total, 10);
    assert_eq!(config.record.max_per_rule, 3);
    assert_eq!(config.record.max_policy, 0);
    assert_eq!(config.record.max_review_days, 180);
    assert_eq!(config.record.deciders, Vec::<String>::new());
    assert_eq!(config.record.trust.low.max_count, Some(1));
    assert_eq!(config.docs_dir, PathBuf::from("docs/defences"));
    assert_eq!(config.coverage.fail_under_lines, 80);
    assert_eq!(config.mutants.mode, MutantsMode::Off);
    assert_eq!(config.mutants.base, "origin/main");
    assert!(config.summary.file.is_none());
    assert!(config.tools.clippy && config.tools.dylint && config.tools.suppression);
    assert!(!config.tools.audit);
    assert_eq!(config.scan_exclude, Vec::<String>::new());
    assert!(
        config.ast_grep.bundled,
        "the bundled ast-grep pack is on by default"
    );
}

#[test]
fn record_accepts_a_bare_path() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), DEFAULT_PATH, "record = \"exceptions.toml\"\n");
    let config = Config::load(dir.path(), None).unwrap();
    assert_eq!(config.record.path, PathBuf::from("exceptions.toml"));
    assert_eq!(config.record.max_total, 10);
}

#[test]
fn record_table_sets_budgets_deciders_and_trust_tiers() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        DEFAULT_PATH,
        "[record]\npath = \"qa/exceptions.toml\"\nmax_total = 8\nmax_per_rule = 4\nmax_policy = 1\n\
         max_review_days = 120\ndeciders = [\"alice\"]\nhistory_base = \"origin/trunk\"\n\
         [record.trust]\nmodels = { big = \"high\", mid = \"medium\" }\n\
         [record.trust.medium]\nmax_review_days = 30\nmax_count = 2\n\
         [[record.trust.cosign]]\nrules = [\"cargo-deny::*\"]\nmin_tier = \"high\"\n",
    );
    let config = Config::load(dir.path(), None).unwrap();
    let record = &config.record;
    assert_eq!(record.path, PathBuf::from("qa/exceptions.toml"));
    assert_eq!(
        (record.max_total, record.max_per_rule, record.max_policy),
        (8, 4, 1)
    );
    assert_eq!(record.max_review_days, 120);
    assert_eq!(record.deciders, vec!["alice".to_owned()]);
    assert_eq!(record.history_base, "origin/trunk");
    assert_eq!(record.tier("big-2"), Tier::High);
    assert_eq!(record.trust.medium.max_count, Some(2));
    assert_eq!(record.trust.cosign.len(), 1);
}

#[test]
fn record_table_rejects_unknown_keys_and_unsafe_policy() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), DEFAULT_PATH, "[record]\nmax_totl = 3\n");
    assert!(Config::load(dir.path(), None).is_err());
    write(
        dir.path(),
        DEFAULT_PATH,
        "[record]\ndeciders = [\"merge-bot\"]\n",
    );
    let error = Config::load(dir.path(), None).unwrap_err().to_string();
    assert!(error.contains("automation"), "{error}");
    write(dir.path(), DEFAULT_PATH, "record = 3\n");
    assert!(Config::load(dir.path(), None).is_err());
}

#[test]
fn bundled_ast_grep_pack_can_be_switched_off() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), DEFAULT_PATH, "[ast_grep]\nbundled = false\n");
    let config = Config::load(dir.path(), None).unwrap();
    assert!(!config.ast_grep.bundled);
    assert!(config.tools.ast_grep, "project rules still run");
    write(dir.path(), DEFAULT_PATH, "[ast_grep]\nbundle = false\n");
    assert!(Config::load(dir.path(), None).is_err());
}

#[test]
fn project_file_overrides_only_what_it_names() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        DEFAULT_PATH,
        "docs_dir = \"doc/rules\"\n[coverage]\nfail_under_lines = 95\n[mutants]\nmode = \"diff\"\n\
         [tools]\naudit = true\n[summary]\nfile = \"AGENTS.md\"\n",
    );
    let config = Config::load(dir.path(), None).unwrap();
    assert_eq!(config.docs_dir, PathBuf::from("doc/rules"));
    assert_eq!(config.coverage.fail_under_lines, 95);
    assert_eq!(config.mutants.mode, MutantsMode::Diff);
    assert_eq!(config.mutants.base, "origin/main");
    assert!(config.tools.audit);
    assert!(config.tools.clippy);
    assert_eq!(config.summary.file, Some(PathBuf::from("AGENTS.md")));
}

#[test]
fn unknown_keys_are_rejected_so_typos_cannot_disable_a_gate() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), DEFAULT_PATH, "[tools]\nclipy = false\n");
    assert!(Config::load(dir.path(), None).is_err());
}

#[test]
fn coverage_threshold_above_one_hundred_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        DEFAULT_PATH,
        "[coverage]\nfail_under_lines = 101\n",
    );
    assert!(Config::load(dir.path(), None).is_err());
}

#[test]
fn explicit_config_path_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Config::load(dir.path(), Some(Path::new("nope.toml"))).is_err());
}
