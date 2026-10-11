//! Cargo manifest inspection and workspace file walking.
#![cfg(test)]

use std::path::{Path, PathBuf};

use rust_qa::manifest::{Level, has_dylint_metadata, lint_settings, workspace_files};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const ROOT_MANIFEST: &str = "[workspace]\nmembers = []\n\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n\n\
[workspace.lints.clippy]\npedantic = { level = \"warn\", priority = -1 }\nunwrap_used = \"deny\"\n";

#[test]
fn workspace_lint_tables_are_read_with_levels_and_lines() {
    let settings = lint_settings(ROOT_MANIFEST, Path::new("Cargo.toml")).unwrap();
    let ids: Vec<(&str, Level, usize)> = settings
        .iter()
        .map(|s| (s.id.as_str(), s.level, s.line))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("unsafe_code", Level::Forbid, 5),
            ("clippy::pedantic", Level::Warn, 8),
            ("clippy::unwrap_used", Level::Deny, 9),
        ]
    );
}

#[test]
fn member_lint_tables_are_read_and_inheritance_is_ignored() {
    let text = "[package]\nname = \"a\"\n[lints]\nworkspace = true\n";
    assert_eq!(
        lint_settings(text, Path::new("a/Cargo.toml")).unwrap(),
        vec![]
    );
    let text = "[package]\nname = \"a\"\n[lints.clippy]\nneedless-pass-by-value = \"allow\"\n\
                [lints.rustdoc]\nbroken_intra_doc_links = \"deny\"\n";
    let settings = lint_settings(text, Path::new("a/Cargo.toml")).unwrap();
    assert_eq!(settings[0].id, "clippy::needless_pass_by_value");
    assert_eq!(settings[0].level, Level::Allow);
    assert_eq!(settings[0].line, 4);
    assert_eq!(settings[0].manifest, PathBuf::from("a/Cargo.toml"));
    assert_eq!(settings[1].id, "rustdoc::broken_intra_doc_links");
}

#[test]
fn unknown_level_is_an_error() {
    assert!(
        lint_settings(
            "[lints.rust]\ndead_code = \"loud\"\n",
            Path::new("Cargo.toml")
        )
        .is_err()
    );
}

#[test]
fn dylint_metadata_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Cargo.toml", ROOT_MANIFEST);
    assert!(!has_dylint_metadata(dir.path()).unwrap());
    write(
        dir.path(),
        "Cargo.toml",
        "[workspace]\n[workspace.metadata.dylint]\nlibraries = [{ path = \"lints/*\" }]\n",
    );
    assert!(has_dylint_metadata(dir.path()).unwrap());
}

#[test]
fn workspace_files_skip_target_git_and_excluded_globs_and_honour_scope() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [
        "src/lib.rs",
        "src/inner/mod.rs",
        "target/debug/build.rs",
        "lib/target/x.rs",
        ".git/hooks/a.rs",
        "fixtures/bad.rs",
        "README.md",
    ] {
        write(dir.path(), rel, "");
    }
    let exclude = rust_qa::glob_set(&["fixtures/*"]).unwrap();
    let is_rs = |p: &Path| p.extension().is_some_and(|e| e == "rs");
    let all = workspace_files(dir.path(), &[], &exclude, is_rs);
    assert_eq!(
        all,
        vec![
            PathBuf::from("src/inner/mod.rs"),
            PathBuf::from("src/lib.rs")
        ]
    );

    let scoped = workspace_files(dir.path(), &[PathBuf::from("src/inner")], &exclude, is_rs);
    assert_eq!(scoped, vec![PathBuf::from("src/inner/mod.rs")]);

    let single = workspace_files(dir.path(), &[PathBuf::from("src/lib.rs")], &exclude, is_rs);
    assert_eq!(single, vec![PathBuf::from("src/lib.rs")]);
}

#[test]
fn workspace_files_honour_gitignore_but_keep_hidden_config_dirs() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [
        "src/lib.rs",
        "scratch/clone/src/lib.rs",
        ".cargo/config.toml",
    ] {
        write(dir.path(), rel, "");
    }
    write(dir.path(), ".gitignore", "scratch/\n");
    let none = rust_qa::glob_set::<&str>(&[]).unwrap();
    let files = workspace_files(dir.path(), &[], &none, |p| {
        p.extension().is_some_and(|e| e == "rs" || e == "toml")
    });
    assert_eq!(
        files,
        vec![
            PathBuf::from(".cargo/config.toml"),
            PathBuf::from("src/lib.rs")
        ]
    );
}

#[test]
fn ignore_files_above_the_root_do_not_hide_the_workspace() {
    let outer = tempfile::tempdir().unwrap();
    write(outer.path(), ".gitignore", "nested/\n");
    write(outer.path(), "nested/.gitignore", "*\n");
    write(outer.path(), ".git/HEAD", "ref: refs/heads/main\n");
    write(
        outer.path(),
        "nested/ws/.git/HEAD",
        "ref: refs/heads/main\n",
    );
    write(outer.path(), "nested/ws/src/lib.rs", "");
    let root = outer.path().join("nested/ws");
    let none = rust_qa::glob_set::<&str>(&[]).unwrap();
    let files = workspace_files(&root, &[], &none, |_| true);
    assert_eq!(files, vec![PathBuf::from("src/lib.rs")]);
}

#[test]
fn dylint_library_entries_are_read_from_the_root_manifest() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[workspace]\n[workspace.metadata.dylint]\nlibraries = [\n  { path = \"lints\" },\n  \
         { git = \"https://example.com/l\", rev = \"abc\", pattern = \"lints/x\" },\n]\n",
    );
    let libraries = rust_qa::manifest::dylint_libraries(dir.path()).unwrap();
    assert_eq!(libraries.len(), 2);
    assert_eq!(libraries[0]["path"].as_str(), Some("lints"));
    assert_eq!(libraries[1]["rev"].as_str(), Some("abc"));
    write(dir.path(), "Cargo.toml", "[workspace]\n");
    assert_eq!(
        rust_qa::manifest::dylint_libraries(dir.path()).unwrap(),
        Vec::<toml::Table>::new()
    );
}
