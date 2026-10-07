//! Built-in identifiers are stable, namespaced and documented (release gate: no dangling ids).
#![cfg(test)]

use std::collections::HashSet;

use rust_qa::builtin;
use rust_qa::docs;

#[test]
fn builtin_identifiers_are_namespaced_and_unique() {
    let mut seen = HashSet::new();
    for id in builtin::ALL {
        assert!(id.starts_with("rqaci."), "{id} is not namespaced");
        assert!(seen.insert(id), "{id} is duplicated");
    }
}

#[test]
fn every_builtin_identifier_has_bundled_documentation() {
    for id in builtin::ALL {
        let doc = docs::bundled(id);
        assert!(doc.is_some(), "{id} has no bundled doc");
    }
}
