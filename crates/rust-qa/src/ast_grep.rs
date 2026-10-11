//! Bundled supplementary ast-grep rule pack.
//!
//! A few cheap single-file rules where syntax provably suffices and no Clippy or rustc lint covers
//! the same ground. The `ast-grep` detector writes the pack to [`PACK_PATH`] and scans with it in
//! addition to the project's own `sgconfig.yml` rules, unless `[ast_grep] bundled = false`.
//! Each rule has a bundled document under `docs/rules/<id>.md`.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use yaml_rust2::{Yaml, YamlLoader};

use crate::Error;
use crate::defences::{field, load_yaml};
use crate::hit::Hit;
use crate::manifest::workspace_files;

/// Where the pack is written before a scan, relative to the workspace root.
pub const PACK_PATH: &str = "target/rust-qa/ast-grep/rqaci-pack.yml";

/// One bundled rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledRule {
    /// Rule id, exactly as ast-grep prints it.
    pub id: &'static str,
    /// The rule's YAML (one document).
    pub yaml: &'static str,
}

/// Every bundled rule. Every file under `rules/ast-grep/` must be listed (enforced by a test).
static PACK: &[BundledRule] = &[
    BundledRule {
        id: "rqaci.ast.hardcoded-temp-path",
        yaml: include_str!("../rules/ast-grep/rqaci.ast.hardcoded-temp-path.yml"),
    },
    BundledRule {
        id: "rqaci.ast.shell-c-command",
        yaml: include_str!("../rules/ast-grep/rqaci.ast.shell-c-command.yml"),
    },
    BundledRule {
        id: "rqaci.ast.tls-verification-disabled",
        yaml: include_str!("../rules/ast-grep/rqaci.ast.tls-verification-disabled.yml"),
    },
];

/// Every bundled rule.
#[must_use]
pub fn bundled() -> &'static [BundledRule] {
    PACK
}

/// Identifiers of the bundled rules.
pub fn bundled_ids() -> impl Iterator<Item = &'static str> {
    PACK.iter().map(|rule| rule.id)
}

/// A bundled rule's message, from its YAML.
#[must_use]
pub fn message(rule: &BundledRule) -> String {
    YamlLoader::load_from_str(rule.yaml)
        .ok()
        .and_then(|docs| {
            docs.first()
                .and_then(|doc| field(doc, "message"))
                .and_then(Yaml::as_str)
                .map(|text| text.trim().to_owned())
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMatch {
    file: String,
    rule_id: String,
    #[serde(default)]
    severity: String,
    message: String,
    #[serde(default)]
    note: Option<String>,
    range: RawRange,
}

#[derive(Deserialize)]
struct RawRange {
    start: RawPosition,
}

#[derive(Deserialize)]
struct RawPosition {
    line: usize,
    column: usize,
}

/// Findings from `ast-grep scan --json=compact`; positions become one-based, and only matches of
/// `error` severity block.
///
/// # Errors
/// Returns the parser's message when the text is not a JSON list of matches, which means the scan
/// did not run.
pub fn parse_matches(json: &str) -> Result<Vec<Hit>, String> {
    let raw: Vec<RawMatch> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(|found| Hit {
            blocking: found.severity == "error",
            rule: found.rule_id,
            path: PathBuf::from(found.file),
            line: found.range.start.line + 1,
            column: found.range.start.column + 1,
            message: found.message.trim().to_owned(),
            note: found
                .note
                .map(|note| note.trim().to_owned())
                .filter(|note| !note.is_empty()),
            rendered: None,
        })
        .collect())
}

/// The `testDir` of each `testConfigs` entry in the root's `sgconfig.yml`.
///
/// # Errors
/// Returns an error when `sgconfig.yml` cannot be read or parsed.
pub fn test_dirs(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let docs = load_yaml(&root.join("sgconfig.yml"))?;
    Ok(docs
        .first()
        .and_then(|doc| field(doc, "testConfigs"))
        .and_then(Yaml::as_vec)
        .map(|configs| {
            configs
                .iter()
                .filter_map(|config| field(config, "testDir").and_then(Yaml::as_str))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default())
}

/// Whether `sgconfig.yml` declares any test directory.
///
/// # Errors
/// Returns an error when `sgconfig.yml` cannot be read or parsed.
pub fn has_tests(root: &Path) -> Result<bool, Error> {
    Ok(!test_dirs(root)?.is_empty())
}

/// Identifiers of the project rules with a test case holding both `valid` and `invalid` code.
///
/// # Errors
/// Returns an error when a test file cannot be read or parsed.
pub fn tested_rules(root: &Path) -> Result<Vec<String>, Error> {
    let none = crate::glob_set::<&str>(&[])?;
    let mut tested = Vec::new();
    for dir in test_dirs(root)? {
        let files = workspace_files(root, std::slice::from_ref(&dir), &none, |p| {
            p.extension().is_some_and(|e| e == "yml" || e == "yaml")
        });
        for file in files {
            for doc in load_yaml(&root.join(&file))? {
                let cases = |key: &str| {
                    field(&doc, key)
                        .and_then(Yaml::as_vec)
                        .is_some_and(|cases| !cases.is_empty())
                };
                if let Some(id) = field(&doc, "id").and_then(Yaml::as_str)
                    && cases("valid")
                    && cases("invalid")
                {
                    tested.push(id.to_owned());
                }
            }
        }
    }
    tested.sort();
    tested.dedup();
    Ok(tested)
}

/// The pack as one multi-document YAML text, as `ast-grep scan --rule` accepts it.
#[must_use]
pub fn pack_yaml() -> String {
    PACK.iter()
        .map(|rule| rule.yaml)
        .collect::<Vec<_>>()
        .join("---\n")
}

/// Writes the pack to [`PACK_PATH`] under `root` and returns its absolute path.
///
/// # Errors
/// Returns [`Error::Write`] when the directory or file cannot be written.
pub fn write_pack(root: &Path) -> Result<PathBuf, Error> {
    let path = root.join(PACK_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&path, pack_yaml()).map_err(|source| Error::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}
