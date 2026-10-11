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
    write(
        root,
        "a/Cargo.toml",
        "[package]\nname = \"a\"\n[lints.clippy]\nexpect_used = \"deny\"\n",
    );
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
        description: "a value of a secret type is printed".to_owned(),
    }]
}

const FAKE_DYLINT: &dyn Fn(&Path) -> Result<Vec<DylintLint>, Error> = &|_| Ok(example_lint());

#[test]
fn enumeration_covers_every_engine_from_configuration() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let find = |id: &str| defences.iter().find(|d| d.id == id);

    assert_eq!(
        find("rqaci.suppression.unrecorded").unwrap().engine,
        Engine::Builtin
    );
    assert_eq!(
        find("rqaci.record.invalid").unwrap().engine,
        Engine::Builtin
    );
    assert_eq!(find("warnings").unwrap().engine, Engine::Rustc);
    assert_eq!(find("unsafe_code").unwrap().level, "forbid");
    assert_eq!(find("clippy::unwrap_used").unwrap().engine, Engine::Clippy);
    assert!(find("clippy::pedantic").unwrap().level.starts_with("warn"));
    assert_eq!(
        find("clippy::expect_used").unwrap().origin,
        "a/Cargo.toml:4"
    );
    assert!(
        find("clippy::needless_pass_by_value").is_none(),
        "allow is a suppression, not a defence"
    );
    assert_eq!(find("example_type_rule").unwrap().engine, Engine::Dylint);
    assert_eq!(find("no-println").unwrap().engine, Engine::AstGrep);
    assert!(find("prefer-x").is_none(), "hint severity does not block");
    assert_eq!(find("cargo-deny").unwrap().engine, Engine::Runner);
    assert!(
        find("rqaci.summary.stale").is_none(),
        "no summary file configured"
    );
}

#[test]
fn disabled_tools_contribute_nothing() {
    let dir = workspace();
    let mut config = Config::default();
    config.tools.clippy = false;
    config.tools.dylint = false;
    config.tools.ast_grep = false;
    let defences = enumerate(dir.path(), &config, FAKE_DYLINT).unwrap();
    assert!(
        defences
            .iter()
            .all(|d| !matches!(d.engine, Engine::Clippy | Engine::Dylint | Engine::AstGrep))
    );
    assert!(defences.iter().all(|d| d.id != "warnings"));
}

#[test]
fn bundled_ast_grep_rules_are_listed_beside_project_rules_unless_switched_off() {
    let dir = workspace();
    let bundled: Vec<&str> = rust_qa::ast_grep::bundled_ids().collect();
    assert_ne!(bundled, Vec::<&str>::new());
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    for id in &bundled {
        let defence = defences.iter().find(|d| d.id == *id).unwrap();
        assert_eq!(defence.engine, Engine::AstGrep);
        assert_eq!(defence.level, "error");
        assert_eq!(defence.origin, "rust-qa bundled ast-grep pack");
    }
    assert!(defences.iter().any(|d| d.id == "no-println"));

    let mut config = Config::default();
    config.ast_grep.bundled = false;
    let defences = enumerate(dir.path(), &config, FAKE_DYLINT).unwrap();
    assert!(defences.iter().all(|d| !bundled.contains(&d.id.as_str())));
    assert!(
        defences.iter().any(|d| d.id == "no-println"),
        "project rules stay active"
    );
}

#[test]
fn bundled_ast_grep_rules_need_no_project_sgconfig() {
    let dir = workspace();
    std::fs::remove_file(dir.path().join("sgconfig.yml")).unwrap();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    for id in rust_qa::ast_grep::bundled_ids() {
        assert!(defences.iter().any(|d| d.id == id), "{id} not listed");
    }
    assert!(defences.iter().all(|d| d.id != "no-println"));
}

#[test]
fn identifiers_are_unique() {
    let dir = workspace();
    write(
        dir.path(),
        "b/Cargo.toml",
        "[package]\nname = \"b\"\n[lints.clippy]\nunwrap_used = \"deny\"\n",
    );
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
        vec![
            ("lib_one", "rule_a", "deny"),
            ("lib_one", "rule_b", "warn"),
            ("lib_two", "rule_c", "deny")
        ]
    );
}

#[test]
fn ast_grep_rules_are_read_from_rule_dirs() {
    let dir = workspace();
    let rules = ast_grep_rules(dir.path()).unwrap();
    let ids: Vec<(&str, &str)> = rules
        .iter()
        .map(|r| (r.id.as_str(), r.severity.as_str()))
        .collect();
    assert_eq!(ids, vec![("no-println", "error"), ("prefer-x", "hint")]);
}

#[test]
fn dylint_list_descriptions_are_kept() {
    let lints = parse_dylint_list("lib_one\n    rule_a    deny    finds the first thing\n");
    assert_eq!(lints[0].description, "finds the first thing");
}

#[test]
fn defences_carry_what_they_guard_where_the_configuration_says() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let find = |id: &str| defences.iter().find(|d| d.id == id).unwrap();
    assert_eq!(
        find("example_type_rule").description,
        "a value of a secret type is printed"
    );
    assert_ne!(find("rqaci.suppression.unrecorded").description, "");
    assert_ne!(find("rqaci.ast.hardcoded-temp-path").description, "");
}

#[test]
fn forced_dylint_lints_are_listed_as_blocking_whatever_their_level() {
    let dir = workspace();
    let mut config = Config::default();
    config.dylint.force_warn = true;
    let defences = enumerate(dir.path(), &config, FAKE_DYLINT).unwrap();
    let lint = defences
        .iter()
        .find(|d| d.id == "example_type_rule")
        .unwrap();
    assert_eq!(lint.level, "forced; every finding blocks");
}

#[test]
fn project_stages_and_the_rules_they_report_are_listed() {
    let dir = workspace();
    let config: Config = toml::from_str(
        r#"
[[stage]]
name = "web-lint"
phase = "detectors"
says = "The web code's own rules"
fix = "Follow each rule's page"
command = ["true"]
output = "findings"
rules = ["echo", '[{"id": "WEB-RULE", "description": "what it finds"}]']
"#,
    )
    .unwrap();
    let defences = enumerate(dir.path(), &config, FAKE_DYLINT).unwrap();
    let stage = defences.iter().find(|d| d.id == "web-lint").unwrap();
    assert_eq!(stage.engine, Engine::Stage);
    assert_eq!(stage.level, "detectors");
    assert_eq!(stage.description, "The web code's own rules");
    let rule = defences.iter().find(|d| d.id == "WEB-RULE").unwrap();
    assert_eq!(rule.engine, Engine::Stage);
    assert_eq!(rule.origin, "stage web-lint");
    assert_eq!(rule.description, "what it finds");
}

#[test]
fn project_ast_grep_rules_bring_the_test_case_defence() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    assert!(defences.iter().any(|d| d.id == "rqaci.rules.untested"));
    std::fs::remove_file(dir.path().join("sgconfig.yml")).unwrap();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    assert!(defences.iter().all(|d| d.id != "rqaci.rules.untested"));
}

#[test]
fn a_rule_message_is_listed_without_its_own_identifier_or_page() {
    assert_eq!(
        rust_qa::defences::terse_message("R-1", "R-1: a thing is wrong (docs/rules/R-1.md)"),
        "a thing is wrong"
    );
    assert_eq!(
        rust_qa::defences::terse_message("R-1", "a thing (in brackets) is wrong"),
        "a thing (in brackets) is wrong"
    );
}

#[test]
fn project_ast_grep_rules_without_an_id_are_named_by_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("rules")).unwrap();
    std::fs::write(root.join("sgconfig.yml"), "ruleDirs: [rules]\n").unwrap();
    std::fs::write(
        root.join("rules/noid.yml"),
        "language: rust\nseverity: error\nmessage: m\nrule:\n  pattern: x\n",
    )
    .unwrap();
    std::fs::write(
        root.join("rules/ok.yml"),
        "id: ok\nlanguage: rust\nseverity: error\nmessage: m\nrule:\n  pattern: x\n",
    )
    .unwrap();
    assert_eq!(
        rust_qa::defences::unnamed_ast_grep_rules(root).unwrap(),
        vec![std::path::PathBuf::from("rules/noid.yml")]
    );
}

#[test]
fn every_lint_from_a_lints_table_carries_its_own_one_line_description() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let find = |id: &str| defences.iter().find(|d| d.id == id).unwrap();
    let unwrap_used = &find("clippy::unwrap_used").description;
    assert!(unwrap_used.contains("unwrap"), "{unwrap_used}");
    assert_ne!(find("unsafe_code").description, "");
    assert_ne!(find("clippy::pedantic").description, "");
    assert_ne!(find("clippy::expect_used").description, "");
}

#[test]
fn the_warnings_defence_says_how_to_list_every_lint_it_makes_blocking() {
    let dir = workspace();
    let defences = enumerate(dir.path(), &Config::default(), FAKE_DYLINT).unwrap();
    let warnings = &defences
        .iter()
        .find(|d| d.id == "warnings")
        .unwrap()
        .description;
    assert!(warnings.contains("rustc -W help"), "{warnings}");
    assert!(warnings.contains("clippy-driver -W help"), "{warnings}");
}

#[test]
fn a_lint_help_listing_gives_lints_and_groups_their_descriptions() {
    let listing = "\nAvailable lint options:\n\nLint checks provided by rustc:\n\n    name  default  meaning\n    ----  -------  -------\n    unsafe-code  allow  usage of `unsafe` code and other potentially unsound constructs\n    clippy::len-zero  warn  checking `.len() == 0`\n\nLint groups provided by rustc:\n\n    name  sub-lints\n    ----  ---------\n    unused  unused-imports, unused-variables\n";
    let help = rust_qa::defences::parse_lint_help(listing);
    assert_eq!(
        help.get("unsafe_code").map(String::as_str),
        Some("usage of `unsafe` code and other potentially unsound constructs")
    );
    assert_eq!(
        help.get("clippy::len_zero").map(String::as_str),
        Some("checking `.len() == 0`")
    );
    assert_eq!(
        help.get("unused").map(String::as_str),
        Some("the lint group of unused-imports, unused-variables")
    );
}
