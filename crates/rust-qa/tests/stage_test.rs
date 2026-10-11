//! Project stages: validation, the files a stage is given, its command lines, and the findings and
//! rules it prints.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::pipeline::Phase;
use rust_qa::stage::{Output, Stage, parse_findings, parse_rules};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn stage(toml_text: &str) -> Stage {
    toml::from_str(toml_text).unwrap()
}

const SHELL: &str = r#"
name = "shellcheck"
phase = "detectors"
says = "ShellCheck over every shell script"
fix = "Quote expansions and check every status, as the finding's page says"
command = ["shellcheck", "{files}"]
files = ["**/*.sh"]
shebangs = ["bash", "sh"]
exclude = ["vendor/**"]
"#;

#[test]
fn a_stage_reads_from_the_configuration() {
    let stage = stage(SHELL);
    assert_eq!(stage.name, "shellcheck");
    assert_eq!(stage.phase, Phase::Detectors);
    assert_eq!(stage.output, Output::Status);
    assert!(stage.dir.is_none());
    assert!(stage.problems().is_empty(), "{:?}", stage.problems());
}

#[test]
fn a_stage_must_say_what_it_checks_how_to_fix_it_and_what_it_runs() {
    let stage = stage("name = \"\"\nphase = \"runners\"\nsays = \" \"\nfix = \"\"\ncommand = []\n");
    let problems = stage.problems().join("\n");
    for expected in ["no name", "`says`", "`fix`", "empty command"] {
        assert!(problems.contains(expected), "{expected}: {problems}");
    }
}

#[test]
fn file_placeholders_need_a_file_selection_and_a_whole_argument() {
    let stage = stage(
        "name = \"x\"\nphase = \"detectors\"\nsays = \"s\"\nfix = \"f\"\n\
         command = [\"tool\", \"--files={files}\"]\nfiles_command = [\"tool\"]\n",
    );
    let problems = stage.problems().join("\n");
    assert!(problems.contains("whole argument"), "{problems}");
    assert!(problems.contains("selects no files"), "{problems}");
}

#[test]
fn files_are_chosen_by_glob_or_by_shebang_within_the_scope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "scripts/build.sh", "echo build\n");
    write(root, "bin/run", "#!/usr/bin/env bash\necho run\n");
    write(root, "bin/tool", "#!/usr/bin/python3\nprint()\n");
    write(root, "bin/plain", "no shebang\n");
    write(root, "vendor/lib.sh", "echo vendored\n");
    let stage = stage(SHELL);
    assert_eq!(
        stage.matching_files(root, &[]).unwrap(),
        vec![PathBuf::from("bin/run"), PathBuf::from("scripts/build.sh")]
    );
    assert_eq!(
        stage
            .matching_files(root, &[PathBuf::from("scripts")])
            .unwrap(),
        vec![PathBuf::from("scripts/build.sh")]
    );
}

#[test]
fn the_files_placeholder_expands_to_one_argument_per_file() {
    let stage = stage(SHELL);
    let files = [PathBuf::from("a.sh"), PathBuf::from("b c.sh")];
    assert_eq!(
        stage.argv(false, &files),
        Some(vec![
            "shellcheck".to_owned(),
            "a.sh".to_owned(),
            "b c.sh".to_owned()
        ])
    );
    assert_eq!(stage.argv(false, &[]), None, "nothing to check is a skip");
}

#[test]
fn a_subset_run_uses_the_files_command_and_a_stage_without_one_runs_whole() {
    let formatter = stage(
        "name = \"prettier\"\nphase = \"format\"\nsays = \"s\"\nfix = \"f\"\n\
         command = [\"prettier\", \"--check\", \".\"]\n\
         files_command = [\"prettier\", \"--check\", \"{files}\"]\nfiles = [\"**/*.ts\"]\n",
    );
    let files = [PathBuf::from("src/a.ts")];
    assert_eq!(
        formatter.argv(false, &files).unwrap().join(" "),
        "prettier --check ."
    );
    assert_eq!(
        formatter.argv(true, &files).unwrap().join(" "),
        "prettier --check src/a.ts"
    );
    let whole = stage(
        "name = \"tsc\"\nphase = \"detectors\"\nsays = \"s\"\nfix = \"f\"\ncommand = [\"tsc\", \"--noEmit\"]\n",
    );
    assert_eq!(whole.argv(true, &files).unwrap().join(" "), "tsc --noEmit");
}

#[test]
fn files_are_passed_relative_to_the_stage_directory() {
    let lint = stage(
        "name = \"x\"\nphase = \"detectors\"\nsays = \"s\"\nfix = \"f\"\ndir = \"web\"\n\
         command = [\"lint\", \"{files}\"]\nfiles = [\"web/**/*.ts\"]\n",
    );
    assert_eq!(
        lint.argv(false, &[PathBuf::from("web/src/a.ts")]),
        Some(vec!["lint".to_owned(), "src/a.ts".to_owned()])
    );
}

#[test]
fn a_stage_documents_itself_with_the_correct_construction() {
    let doc = stage(SHELL).doc();
    assert!(doc.starts_with("# shellcheck\n"), "{doc}");
    assert!(doc.contains("ShellCheck over every shell script"));
    assert!(doc.contains("\n## How to fix correctly\n\nQuote expansions"));
    assert!(doc.contains("`shellcheck {files}`"));
}

#[test]
fn findings_print_as_json_relative_to_the_stage_directory() {
    let json = r#"[{"file":"src/a.ts","line":4,"rule":"web-rule","message":"m","note":"n"},
                   {"file":"b.yml","line":1,"rule":"yaml-rule","message":"m2"}]"#;
    let hits = parse_findings(json, Some(Path::new("web"))).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].path, PathBuf::from("web/src/a.ts"));
    assert_eq!((hits[0].line, hits[0].rule.as_str()), (4, "web-rule"));
    assert_eq!(hits[0].note.as_deref(), Some("n"));
    assert!(hits[1].blocking);
    assert!(parse_findings("not json", None).is_err());
}

#[test]
fn rules_print_as_json_with_a_description_each() {
    let rules = parse_rules(r#"[{"id":"web-rule","description":"what it finds"}]"#).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "web-rule");
    assert_eq!(rules[0].description, "what it finds");
    assert!(parse_rules("[{\"id\": 3}]").is_err());
}
