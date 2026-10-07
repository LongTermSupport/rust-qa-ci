//! Bundled supplementary ast-grep rule pack.
//!
//! A few cheap single-file rules where syntax provably suffices and no Clippy or rustc lint covers
//! the same ground. The `ast-grep` detector writes the pack to [`PACK_PATH`] and scans with it in
//! addition to the project's own `sgconfig.yml` rules, unless `[ast_grep] bundled = false`.
//! Each rule has a bundled document under `docs/rules/<id>.md`.

use std::path::{Path, PathBuf};

use crate::Error;

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
