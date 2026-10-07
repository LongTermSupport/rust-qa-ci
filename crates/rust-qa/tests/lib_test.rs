//! Crate-level helpers.
#![cfg(test)]

use rust_qa::{DBF_POINTER, Error, read_toml};

#[test]
fn failure_pointer_names_the_method_and_links_it() {
    assert!(DBF_POINTER.contains("Defence Before Fix"));
    assert!(DBF_POINTER.contains("https://defence-before-fix.github.io/"));
    assert!(DBF_POINTER.contains("rust-qa rule-doc"));
}

#[test]
fn read_toml_reports_the_failing_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "not = [valid").unwrap();
    let err = read_toml::<toml::Value>(&path).unwrap_err();
    assert!(matches!(err, Error::Toml { .. }));
    assert!(err.to_string().contains("bad.toml"));
}

#[test]
fn read_toml_reports_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let err = read_toml::<toml::Value>(&dir.path().join("absent.toml")).unwrap_err();
    assert!(matches!(err, Error::Read { .. }));
}
