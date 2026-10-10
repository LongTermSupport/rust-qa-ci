//! Suppression guard: every suppression route is found and must match a project-record exception.
//!
//! Fixture sources spell the allow attribute `LINTOFF(`, expanded by [`src`], so this file never
//! carries a literal suppression itself.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::builtin::{RECORD_STALE, SUPPRESSION_ALLOW, SUPPRESSION_UNRECORDED};
use rust_qa::record::{Covers, Exception, Record};
use rust_qa::suppression::{Route, Suppression, check, scan, stale};

fn src(text: &str) -> String {
    text.replace("LINTOFF(", "allow(")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, src(text)).unwrap();
}

const LIB: &str = r#"#![expect(clippy::module_name_repetitions, reason = "names mirror the spec")]

#[LINTOFF(dead_code)]
fn a() {}

#[cfg_attr(test, LINTOFF(clippy::unwrap_used, clippy::expect_used))]
fn b() {}

macro_rules! m {
    () => {
        #[expect(unused_variables, reason = "macro")]
        fn c() {}
    };
}

// ast-grep-ignore: no-println
fn d() {}

#[warn(missing_docs)]
#[doc = "LINTOFF(dead_code) in a doc string is not a suppression"]
fn e() {}
"#;

fn no_exclude() -> globset::GlobSet {
    rust_qa::glob_set::<&str>(&[]).unwrap()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/lib.rs", LIB);
    write(
        root,
        "target/debug/gen.rs",
        "#[LINTOFF(dead_code)] fn g() {}\n",
    );
    write(
        root,
        "fixtures/ui/bad.rs",
        "#[LINTOFF(dead_code)] fn g() {}\n",
    );
    write(
        root,
        "Cargo.toml",
        "[workspace]\n[workspace.lints.rust]\ndead_code = { level = \"allow\", priority = 1 }\n\
         unsafe_code = \"forbid\"\n[lints.clippy]\nneedless-pass-by-value = \"allow\"\n",
    );
    write(
        root,
        ".cargo/config.toml",
        "[build]\nrustflags = [\"-A\", \"unused\", \"-Aclippy::pedantic\", \"--cap-lints=warn\", \"-Dwarnings\"]\n\
         [target.x86_64-unknown-linux-gnu]\nrustflags = \"--allow=dead_code\"\n",
    );
    write(
        root,
        "clippy.toml",
        "allow-unwrap-in-tests = true\nallow-expect-in-tests = false\nmsrv = \"1.99\"\n",
    );
    write(
        root,
        "deny.toml",
        "[advisories]\nignore = [\"RUSTSEC-2020-0001\", { id = \"RUSTSEC-2021-0002\", reason = \"x\" }]\n",
    );
    dir
}

fn found(suppressions: &[Suppression]) -> Vec<(String, String, usize, Route)> {
    suppressions
        .iter()
        .map(|s| {
            (
                s.rule.clone(),
                s.path.display().to_string(),
                s.line,
                s.route,
            )
        })
        .collect()
}

#[test]
fn scan_finds_every_suppression_route() {
    let dir = fixture();
    let exclude = rust_qa::glob_set(&[concat!("fixtures", "/", "*")]).unwrap();
    let result = scan(dir.path(), &[], &exclude).unwrap();
    assert_eq!(result.unparseable, vec![]);
    let got = found(&result.suppressions);
    let expected: Vec<(String, String, usize, Route)> = [
        (".cargo/config.toml", "unused", 2, Route::Rustflags),
        (
            ".cargo/config.toml",
            "clippy::pedantic",
            2,
            Route::Rustflags,
        ),
        (
            ".cargo/config.toml",
            "rustflags::cap-lints",
            2,
            Route::Rustflags,
        ),
        (".cargo/config.toml", "dead_code", 4, Route::Rustflags),
        ("Cargo.toml", "dead_code", 3, Route::ManifestLints),
        (
            "Cargo.toml",
            "clippy::needless_pass_by_value",
            6,
            Route::ManifestLints,
        ),
        (
            "clippy.toml",
            "clippy.toml::allow-unwrap-in-tests",
            1,
            Route::ClippyConfig,
        ),
        (
            "deny.toml",
            "cargo-deny::RUSTSEC-2020-0001",
            2,
            Route::DenyIgnore,
        ),
        (
            "deny.toml",
            "cargo-deny::RUSTSEC-2021-0002",
            2,
            Route::DenyIgnore,
        ),
        (
            "src/lib.rs",
            "clippy::module_name_repetitions",
            1,
            Route::Attribute,
        ),
        ("src/lib.rs", "dead_code", 3, Route::AllowAttribute),
        (
            "src/lib.rs",
            "clippy::unwrap_used",
            6,
            Route::AllowAttribute,
        ),
        (
            "src/lib.rs",
            "clippy::expect_used",
            6,
            Route::AllowAttribute,
        ),
        ("src/lib.rs", "unused_variables", 11, Route::MacroAttribute),
        (
            "src/lib.rs",
            "ast-grep::no-println",
            16,
            Route::AstGrepIgnore,
        ),
    ]
    .into_iter()
    .map(|(path, rule, line, route)| (rule.to_owned(), path.to_owned(), line, route))
    .collect();
    assert_eq!(got, expected);
}

#[test]
fn bare_ast_grep_ignore_suppresses_every_rule() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.rs", "fn f() {} // ast-grep-ignore\n");
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    assert_eq!(found(&result.suppressions)[0].0, "ast-grep::*");
}

#[test]
fn markers_inside_string_literals_are_not_suppressions() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.rs",
        "const A: &str = \"// ast-grep-ignore\";\nconst B: &str = r#\"#[LINTOFF(dead_code)] // ast-grep-ignore\"#;\n\
         const C: char = '\"';\nfn f() {} /* ast-grep-ignore: real-rule */\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let got = found(&result.suppressions);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "ast-grep::real-rule");
    assert_eq!(got[0].2, 4);
}

#[test]
fn doc_comments_and_prose_mentioning_the_marker_are_not_directives() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.rs",
        "//! ast-grep-ignore in crate docs\n/// An `// ast-grep-ignore` comment.\nfn f() {}\n\
         // explains ast-grep-ignore usage\nfn g() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    assert_eq!(found(&result.suppressions), vec![]);
}

#[test]
fn scan_honours_scope() {
    let dir = fixture();
    write(
        dir.path(),
        "other/x.rs",
        "#[LINTOFF(dead_code)] fn x() {}\n",
    );
    let result = scan(dir.path(), &[PathBuf::from("other")], &no_exclude()).unwrap();
    assert_eq!(found(&result.suppressions).len(), 1);
    assert_eq!(result.suppressions[0].path, PathBuf::from("other/x.rs"));
}

#[test]
fn unparseable_rust_is_reported_not_skipped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "broken.rs", "fn (\n");
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    assert_eq!(result.unparseable.len(), 1);
    let findings = check(
        &result,
        &Record::default(),
        Path::new("qa/record.toml"),
        true,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, SUPPRESSION_UNRECORDED);
}

fn exception(rule: &str, path: &str) -> Exception {
    Exception {
        rule: rule.to_owned(),
        path: path.to_owned(),
        justification: "A long enough and specific justification for the test.".to_owned(),
        ..Exception::default()
    }
}

fn record(exceptions: Vec<Exception>) -> Record {
    Record {
        ratchet: None,
        exceptions,
    }
}

#[test]
fn allow_attributes_fail_even_when_recorded_and_expect_is_preferred() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/a.rs",
        "#[LINTOFF(dead_code)]\nfn a() {}\n#[expect(unused, reason = \"r\")]\nfn b() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let rec = record(vec![
        exception("dead_code", "src/a.rs"),
        exception("unused", "src/a.rs"),
    ]);
    let findings = check(&result, &rec, Path::new("qa/record.toml"), true);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, SUPPRESSION_ALLOW);
    assert_eq!(findings[0].line, 1);
    assert!(findings[0].message.contains("expect(dead_code"));
}

#[test]
fn an_allow_can_be_recorded_as_its_own_exception() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/gen.rs",
        "#[LINTOFF(dead_code)]\nfn a() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let rec = record(vec![
        exception("dead_code", "src/gen.rs"),
        exception(SUPPRESSION_ALLOW, "src/gen.rs"),
    ]);
    assert_eq!(
        check(&result, &rec, Path::new("qa/record.toml"), true),
        vec![]
    );
    assert_eq!(stale(&result, &rec), Vec::<usize>::new());
}

#[test]
fn stale_lists_exceptions_that_cover_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/a.rs",
        "#[expect(dead_code, reason = \"r\")]\nfn a() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let rec = record(vec![
        exception("unused", "src/a.rs"),
        exception("dead_code", "src/a.rs"),
        exception("dead_code", "src/b.rs"),
    ]);
    assert_eq!(stale(&result, &rec), vec![0, 2]);
}

#[test]
fn recorded_suppressions_pass_and_unrecorded_ones_fail() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/a.rs",
        "#[expect(dead_code, reason = \"r\")]\nfn a() {}\n#[expect(clippy::unwrap_used, reason = \"r\")]\nfn b() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let record = record(vec![exception("dead_code", concat!("src", "/", "*.rs"))]);
    let findings = check(&result, &record, Path::new("qa/record.toml"), true);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, SUPPRESSION_UNRECORDED);
    assert_eq!(findings[0].path, PathBuf::from("src/a.rs"));
    assert_eq!(findings[0].line, 3);
    assert!(findings[0].message.contains("clippy::unwrap_used"));
}

#[test]
fn exceptions_matching_nothing_are_stale_only_on_full_scans() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", "fn a() {}\n");
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let record = record(vec![exception("dead_code", "src/a.rs")]);
    let full = check(&result, &record, Path::new("qa/record.toml"), true);
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].rule, RECORD_STALE);
    assert_eq!(
        check(&result, &record, Path::new("qa/record.toml"), false),
        vec![]
    );
}

#[test]
fn a_findings_exception_licenses_no_suppression_and_is_never_stale_here() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/a.rs",
        "#[expect(dead_code, reason = \"r\")]\nfn a() {}\n",
    );
    let result = scan(dir.path(), &[], &no_exclude()).unwrap();
    let mut covering = exception("dead_code", "src/a.rs");
    covering.covers = Covers::Findings;
    let mut elsewhere = exception("my_lint", "src/b.rs");
    elsewhere.covers = Covers::Findings;
    let rec = record(vec![covering, elsewhere]);
    let findings = check(&result, &rec, Path::new("qa/record.toml"), true);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].rule, SUPPRESSION_UNRECORDED);
    assert_eq!(stale(&result, &rec), Vec::<usize>::new());
}
