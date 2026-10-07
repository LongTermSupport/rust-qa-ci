//! Defence enumeration is derived from active configuration, never hand-maintained.
#![cfg(test)]

use std::path::Path;

use rust_qa::Error;
use rust_qa::config::Config;
use rust_qa::defences::{DylintLint, Engine, ast_grep_rules, enumerate, parse_dylint_list};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\"]\n[workspace.metadata.dylint]\nlibraries = [{ path = \"lints\" }]\n\
         [workspace.lints.rust]\nunsafe_code = \"forbid\"\n[workspace.lints.clippy]\n\
         pedantic = { level = \"warn\", priority = -1 }\nunwrap_used = \"deny\"\nneedless_pass_by_value = \"allow\"\n",
    );
    write(root, "a/Cargo.toml", "[package]\nname = \"a\"\n[lints.clippy]\nexpect_used = \"deny\"\n");
    write(root, "sgconfig.yml", "ruleDirs:\n  - qa/rules\n");
    write(
        root,
        "qa/rules/style.yml",
        "id: no-println\nlanguage: rust\nseverity: error\nrule:\n  pattern: println!($$$)\n---\n\
         id: prefer-x\nlanguage: rust\nseverity: hint\nrule:\n  pattern: y\n",
    );
    write(root, "deny.toml", "[advisories]\n");
    dir
}

fn example_lint() -> Vec<DylintLint> {
    vec![DylintLint {
        library: "example_lints".to_owned(),
        name: "example_type_rule".to_owned(),
        level: "deny".to_owned(),
    }]
}

const FAKE_DYLINT:&dyn Fn(&Path) -> Result<Vec<DylintLint>, Error> = &|_| Ok(example_lint());

#[test]
fn enumeration_covers_every_engine_from_configuration() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let find = |id: &str| defences.iter().find(|d| d.id == id);

    assert_eq!(find("rqaci.suppression.unrecorded").unwrap().engine, Engine::Builtin);
    assert_eq!(find("rqaci.record.invalid").unwrap().engine, Engine::Builtin);
    assert_eq!(find("warnings").unwrap().engine, Engine::Rustc);
    assert_eq!(find("unsafe_code").unwrap().level, "forbid");
    assert_eq!(find("clippy::unwrap_used").unwrap().engine, Engine::Clippy);
    assert!(find("clippy::pedantic").unwrap().level.starts_with("warn"));
    assert_eq!(find("clippy::expect_used").unwrap().origin, "a/Cargo.toml:4");
    assert!(find("clippy::needless_pass_by_value").is_none(), "allow is a suppression, not a defence");
    assert_eq!(find("example_type_rule").unwrap().engine, Engine::Dylint);
    assert_eq!(find("no-println").unwrap().engine, Engine::AstGrep);
    assert!(find("prefer-x").is_none(), "hint severity does not block");
    assert_eq!(find("cargo-deny").unwrap().engine, Engine::Runner);
    assert!(find("rqaci.summary.stale").is_none(), "no summary file configured");
}

#[test]
fn disabled_tools_contribute_nothing() {
    let dir = workspace();
    let mut config = Config::default();
    config.tools.clippy = false;
    config.tools.dylint = false;
    config.tools.ast_grep = false;
    let defences = enumerate(dir.path(), &config, FAKE_DYLINT).unwrap();
    assert!(defences.iter().all(|d| !matches!(d.engine, Engine::Clippy | Engine::Dylint | Engine::AstGrep)));
    assert!(defences.iter().all(|d| d.id != "warnings"));
}

#[test]
fn identifiers_are_unique() {
    let dir = workspace();
    write(dir.path(), "b/Cargo.toml", "[package]\nname = \"b\"\n[lints.clippy]\nunwrap_used = \"deny\"\n");
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let mut ids: Vec<&str> = defences.iter().map(|d| d.id.as_str()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before);
}

#[test]
fn dylint_list_output_is_parsed_per_library() {
    let output = "lib_one\n    rule_a    deny    first rule\n    rule_b    warn    second\n\nlib_two\n    rule_c    deny    third\n";
    let lints = parse_dylint_list(output);
    let got: Vec<(&str, &str, &str)> = lints
        .iter()
        .map(|l| (l.library.as_str(), l.name.as_str(), l.level.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![("lib_one", "rule_a", "deny"), ("lib_one", "rule_b", "warn"), ("lib_two", "rule_c", "deny")]
    );
}

#[test]
fn ast_grep_rules_are_read_from_rule_dirs() {
    let dir = workspace();
    let rules = ast_grep_rules(dir.path()).unwrap();
    let ids: Vec<(&str, &str)> = rules.iter().map(|r| (r.id.as_str(), r.severity.as_str())).collect();
    assert_eq!(ids, vec![("no-println", "error"), ("prefer-x", "hint")]);
}
