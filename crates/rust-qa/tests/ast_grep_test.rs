//! Bundled ast-grep rule pack: integrity, documentation, and red-proof fixtures run through the
//! installed `ast-grep` (each rule fires on a violating snippet and stays quiet on a clean one).
#![cfg(test)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use rust_qa::ast_grep::{self, PACK_PATH};
use rust_qa::docs;
use yaml_rust2::{Yaml, YamlLoader};

/// Red-proof fixtures: (rule id, violating snippets, clean snippets). Each snippet is a whole file.
const CASES: &[(&str, &[&str], &[&str])] = &[
    (
        "rqaci.ast.shell-c-command",
        &[
            "use std::process::Command;\nfn f() { let _ = Command::new(\"sh\").arg(\"-c\").arg(\"ls\").status(); }\n",
            "fn f(cmd: &str) { let _ = std::process::Command::new(\"bash\").current_dir(\".\").args([\"-c\", cmd]).output(); }\n",
            "use std::process::Command;\nfn f() { let _ = Command::new(\"bash\").args(&[\"-lc\", \"ls\"]).output(); }\n",
            "async fn f() { let _ = tokio::process::Command::new(\"zsh\").env(\"A\", \"b\").args(vec![\"-ec\", \"ls\"]).output().await; }\n",
        ],
        &[
            "use std::process::Command;\nfn f() { let _ = Command::new(\"ls\").arg(\"-c\").status(); }\n",
            "use std::process::Command;\nfn f() { let _ = Command::new(\"sh\").arg(\"scripts/build.sh\").status(); }\n",
            "use std::process::Command;\nfn f() { let _ = Command::new(\"git\").args([\"commit\", \"-m\", \"msg\"]).status(); }\n",
            "use std::process::Command;\nfn f(name: &str) { let _ = Command::new(\"printf\").arg(\"%s\").arg(name).status(); }\n",
        ],
    ),
    (
        "rqaci.ast.hardcoded-temp-path",
        &[
            "fn f() { let _ = std::fs::write(\"/tmp/report.txt\", b\"x\"); }\n",
            "fn f() -> std::path::PathBuf { std::path::Path::new(\"/tmp\").join(\"cache\") }\n",
            "const SCRATCH: &str = r\"/var/tmp/scratch\";\n",
            "const SCRATCH: &str = r#\"/tmp/scratch\"#;\n",
        ],
        &[
            "fn f() -> std::path::PathBuf { std::env::temp_dir().join(\"cache\") }\n",
            "fn f() { let dir = tempfile::tempdir().unwrap(); let _ = dir.path().join(\"report.txt\"); }\n",
            "const NAMES: [&str; 3] = [\"/tmpfs\", \"tmp/relative\", \"/srv/tmp\"];\n",
            "// Writes under /tmp would be shared; this comment is not code.\nfn f() {}\n",
        ],
    ),
    (
        "rqaci.ast.tls-verification-disabled",
        &[
            "fn f() { let _ = reqwest::Client::builder().danger_accept_invalid_certs(true).build(); }\n",
            "fn f(b: native_tls::TlsConnectorBuilder) { b.danger_accept_invalid_hostnames( true ); }\n",
        ],
        &[
            "fn f() { let _ = reqwest::Client::builder().danger_accept_invalid_certs(false).build(); }\n",
            "fn f() { let _ = reqwest::Client::builder().add_root_certificate(load_ca()).build(); }\n",
        ],
    ),
];

/// Rule ids `ast-grep` reports for `source`, scanned with the bundled pack only.
fn scan(source: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let pack = ast_grep::write_pack(dir.path()).unwrap();
    std::fs::write(dir.path().join("snippet.rs"), source).unwrap();
    let output = Command::new("ast-grep")
        .args(["scan", "--json=compact", "--rule"])
        .arg(&pack)
        .arg("snippet.rs")
        .current_dir(dir.path())
        .output()
        .expect("ast-grep is installed (scripts/install-tools.sh)");
    let matches: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "unparseable ast-grep output ({e}): {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    matches
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["ruleId"].as_str().unwrap().to_owned())
        .collect()
}

fn yaml(text: &str) -> Yaml {
    let mut docs = YamlLoader::load_from_str(text).unwrap();
    assert_eq!(docs.len(), 1, "one rule per bundled file");
    docs.remove(0)
}

#[test]
fn every_violating_snippet_fires_its_rule_and_no_other() {
    for (id, violating, _) in CASES {
        for source in *violating {
            let ids = scan(source);
            assert!(!ids.is_empty(), "{id} did not fire on:\n{source}");
            assert!(
                ids.iter().all(|got| got == id),
                "expected only {id}, got {ids:?} on:\n{source}"
            );
        }
    }
}

#[test]
fn every_clean_snippet_is_quiet() {
    for (id, _, clean) in CASES {
        for source in *clean {
            assert_eq!(
                scan(source),
                Vec::<String>::new(),
                "pack fired on a clean {id} snippet:\n{source}"
            );
        }
    }
}

#[test]
fn every_bundled_rule_has_red_proof_fixtures() {
    let bundled: BTreeSet<&str> = ast_grep::bundled_ids().collect();
    let proven: BTreeSet<&str> = CASES
        .iter()
        .filter(|(_, bad, good)| !bad.is_empty() && !good.is_empty())
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(bundled, proven);
    assert!((2..=3).contains(&bundled.len()), "the pack stays small");
}

#[test]
fn an_ignore_comment_silences_a_bundled_rule() {
    let source = "fn f() {\n    // ast-grep-ignore: rqaci.ast.hardcoded-temp-path\n    let _ = std::fs::write(\"/tmp/x\", b\"\");\n}\n";
    assert_eq!(scan(source), Vec::<String>::new());
}

#[test]
fn bundled_rules_are_namespaced_blocking_rust_rules_whose_id_matches_the_listing() {
    for rule in ast_grep::bundled() {
        assert!(
            rule.id.starts_with("rqaci.ast."),
            "{} not namespaced",
            rule.id
        );
        let doc = yaml(rule.yaml);
        assert_eq!(doc["id"].as_str(), Some(rule.id));
        assert_eq!(doc["language"].as_str(), Some("rust"), "{}", rule.id);
        assert_eq!(doc["severity"].as_str(), Some("error"), "{}", rule.id);
        let note = doc["note"].as_str().unwrap_or_default();
        assert!(
            note.contains(&format!("rust-qa rule-doc {}", rule.id)),
            "{}: note must point at its doc",
            rule.id
        );
    }
}

#[test]
fn every_rule_file_on_disk_is_bundled() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("rules/ast-grep");
    let mut on_disk = BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let id = yaml(&text)["id"].as_str().unwrap().to_owned();
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{id}.yml")
        );
        let rule = ast_grep::bundled().iter().find(|r| r.id == id);
        assert_eq!(
            rule.map(|r| r.yaml),
            Some(text.as_str()),
            "{id} not bundled"
        );
        on_disk.insert(id);
    }
    assert_eq!(on_disk.len(), ast_grep::bundled().len());
}

#[test]
fn every_bundled_rule_documents_why_syntax_suffices() {
    for id in ast_grep::bundled_ids() {
        let doc = docs::bundled(id).unwrap_or_else(|| panic!("{id} has no bundled doc"));
        assert!(doc.contains("\n## Why syntax suffices\n"), "{id}");
        assert!(doc.contains("\n## Suppression\n"), "{id}");
        assert!(
            doc.contains(&format!("ast-grep::{id}")),
            "{id}: doc must name the record identifier"
        );
    }
}

#[test]
fn the_pack_is_written_under_target_as_one_multi_document_file() {
    let dir = tempfile::tempdir().unwrap();
    let pack = ast_grep::write_pack(dir.path()).unwrap();
    assert_eq!(pack, dir.path().join(PACK_PATH));
    let text = std::fs::read_to_string(pack).unwrap();
    let docs = YamlLoader::load_from_str(&text).unwrap();
    let ids: Vec<&str> = docs.iter().filter_map(|d| d["id"].as_str()).collect();
    assert_eq!(ids, ast_grep::bundled_ids().collect::<Vec<_>>());
}
