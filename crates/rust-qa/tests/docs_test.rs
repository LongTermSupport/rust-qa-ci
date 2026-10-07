//! Offline identifier-to-documentation resolution.
#![cfg(test)]

use std::path::Path;

use rust_qa::docs::{self, Resolver, Source, doc_file_name};

#[test]
fn bundled_docs_cover_builtin_ids_groups_and_the_warnings_lint() {
    for id in [
        "rqaci.suppression.unrecorded",
        "warnings",
        "clippy::pedantic",
        "clippy::all",
    ] {
        assert!(docs::bundled(id).is_some(), "{id} missing");
    }
}

#[test]
fn every_bundled_doc_names_its_id_and_states_the_correct_construction() {
    let ids: Vec<&str> = docs::bundled_ids().collect();
    assert!(ids.len() > 1);
    for id in ids {
        let text = docs::bundled(id).unwrap();
        assert!(
            text.starts_with(&format!("# {id}\n")),
            "{id}: first line must be `# {id}`"
        );
        assert!(
            text.contains("\n## How to fix correctly\n"),
            "{id}: no correct-construction section"
        );
    }
}

#[test]
fn every_file_under_docs_rules_is_bundled() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/rules");
    let mut on_disk = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let id = text.lines().next().unwrap().strip_prefix("# ").unwrap();
        assert_eq!(
            docs::bundled(id),
            Some(text.as_str()),
            "{} is not bundled",
            path.display()
        );
        on_disk += 1;
    }
    assert_eq!(docs::bundled_ids().count(), on_disk);
}

#[test]
fn doc_file_names_replace_path_separators() {
    assert_eq!(
        doc_file_name("clippy::unwrap_used"),
        "clippy.unwrap_used.md"
    );
    assert_eq!(doc_file_name("no-println"), "no-println.md");
    assert_eq!(
        doc_file_name("rqaci.record.invalid"),
        "rqaci.record.invalid.md"
    );
}

#[test]
fn project_docs_take_precedence_over_bundled_docs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs/defences")).unwrap();
    std::fs::write(
        dir.path().join("docs/defences/rqaci.record.invalid.md"),
        "# rqaci.record.invalid\nproject text\n",
    )
    .unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    let doc = resolver.resolve("rqaci.record.invalid").unwrap().unwrap();
    assert!(matches!(doc.source, Source::Project(_)));
    assert!(doc.text.contains("project text"));
}

#[test]
fn bundled_doc_resolves_without_project_docs() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    let doc = resolver.resolve("rqaci.record.stale").unwrap().unwrap();
    assert_eq!(doc.source, Source::Bundled);
}

#[test]
fn ast_grep_record_identifiers_resolve_to_the_rule_doc() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    for id in rust_qa::ast_grep::bundled_ids() {
        let doc = resolver
            .resolve(&format!("ast-grep::{id}"))
            .unwrap()
            .unwrap();
        assert_eq!(doc.source, Source::Bundled);
        assert!(doc.text.starts_with(&format!("# {id}\n")));
    }
    std::fs::create_dir_all(dir.path().join("docs/defences")).unwrap();
    std::fs::write(
        dir.path().join("docs/defences/no-println.md"),
        "# no-println\n",
    )
    .unwrap();
    let doc = resolver.resolve("ast-grep::no-println").unwrap().unwrap();
    assert!(matches!(doc.source, Source::Project(_)));
    assert!(
        resolver
            .resolve("ast-grep::undocumented")
            .unwrap()
            .is_none()
    );
}

#[test]
fn clippy_lints_resolve_offline_from_the_installed_clippy() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    let doc = resolver.resolve("clippy::unwrap_used").unwrap().unwrap();
    assert_eq!(doc.source, Source::ClippyExplain);
    assert!(doc.text.to_lowercase().contains("unwrap"));
    assert!(
        resolver
            .resolve("clippy::no_such_lint_anywhere")
            .unwrap()
            .is_none()
    );
}

#[test]
fn rustc_lints_resolve_offline_from_the_installed_toolchain() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    let doc = resolver.resolve("unsafe_code").unwrap().unwrap();
    assert!(matches!(
        doc.source,
        Source::RustcBook(_) | Source::RustcHelp
    ));
    assert!(doc.text.contains("unsafe"));
}

#[test]
fn unknown_identifiers_do_not_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = Resolver::new(dir.path(), Path::new("docs/defences"));
    assert!(resolver.resolve("no_such_rule_anywhere").unwrap().is_none());
    assert!(resolver.resolve("vendor::thing").unwrap().is_none());
}
