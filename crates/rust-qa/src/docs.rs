//! Offline identifier-to-documentation resolution.
//!
//! Order: project document (`<docs_dir>/<id>.md`), bundled document, the installed Clippy
//! (`cargo clippy --explain`), then the installed rustc (lint listing in the `rust-docs` component,
//! else the one-line description from `rustc -W help`). Nothing is fetched from the network.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// Bundled rule documents: (identifier, markdown). Every file under `docs/rules/` must be listed
/// (enforced by a test), and each file's first line is `# <identifier>`.
static BUNDLED: &[(&str, &str)] = &[
    ("cargo-audit", include_str!("../docs/rules/cargo-audit.md")),
    ("cargo-deny", include_str!("../docs/rules/cargo-deny.md")),
    (
        "cargo-machete",
        include_str!("../docs/rules/cargo-machete.md"),
    ),
    ("coverage", include_str!("../docs/rules/coverage.md")),
    ("mutants", include_str!("../docs/rules/mutants.md")),
    ("tests", include_str!("../docs/rules/tests.md")),
    ("clippy::all", include_str!("../docs/rules/clippy.all.md")),
    (
        "clippy::cargo",
        include_str!("../docs/rules/clippy.cargo.md"),
    ),
    (
        "clippy::nursery",
        include_str!("../docs/rules/clippy.nursery.md"),
    ),
    (
        "clippy::pedantic",
        include_str!("../docs/rules/clippy.pedantic.md"),
    ),
    (
        "clippy::restriction",
        include_str!("../docs/rules/clippy.restriction.md"),
    ),
    (
        "rqaci.record.invalid",
        include_str!("../docs/rules/rqaci.record.invalid.md"),
    ),
    (
        "rqaci.record.stale",
        include_str!("../docs/rules/rqaci.record.stale.md"),
    ),
    (
        "rqaci.summary.stale",
        include_str!("../docs/rules/rqaci.summary.stale.md"),
    ),
    (
        "rqaci.suppression.unrecorded",
        include_str!("../docs/rules/rqaci.suppression.unrecorded.md"),
    ),
    (
        "rqaci_sensitive_debug_derive",
        include_str!("../docs/rules/rqaci_sensitive_debug_derive.md"),
    ),
    ("warnings", include_str!("../docs/rules/warnings.md")),
];

/// Bundled document for `id`, if any.
#[must_use]
pub fn bundled(id: &str) -> Option<&'static str> {
    BUNDLED
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, text)| *text)
}

/// Identifiers with a bundled document.
pub fn bundled_ids() -> impl Iterator<Item = &'static str> {
    BUNDLED.iter().map(|(id, _)| *id)
}

/// File name of the project document for `id` (`::` becomes `.`).
#[must_use]
pub fn doc_file_name(id: &str) -> String {
    format!("{}.md", id.replace("::", "."))
}

/// Where a document came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Project-authored document.
    Project(PathBuf),
    /// Shipped inside `rust-qa`.
    Bundled,
    /// `cargo clippy --explain` from the installed toolchain.
    ClippyExplain,
    /// Lint listing of the installed `rust-docs` component.
    RustcBook(PathBuf),
    /// One-line description from `rustc -W help`.
    RustcHelp,
}

/// A resolved document.
#[derive(Debug, Clone)]
pub struct Doc {
    /// Origin.
    pub source: Source,
    /// Markdown or plain text.
    pub text: String,
}

/// Resolves identifiers for one workspace.
#[derive(Debug, Clone)]
pub struct Resolver {
    root: PathBuf,
    docs_dir: PathBuf,
}

impl Resolver {
    /// Resolver for the workspace at `root` with project documents in `docs_dir` (relative to root).
    #[must_use]
    pub fn new(root: &Path, docs_dir: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            docs_dir: docs_dir.to_path_buf(),
        }
    }

    /// Resolves `id`; `Ok(None)` means the identifier is dangling.
    ///
    /// # Errors
    /// Returns an error when a project document exists but cannot be read.
    pub fn resolve(&self, id: &str) -> Result<Option<Doc>, Error> {
        let id = id.trim();
        let project = self.docs_dir.join(doc_file_name(id));
        let absolute = self.root.join(&project);
        if absolute.is_file() {
            return Ok(Some(Doc {
                source: Source::Project(project),
                text: crate::read_to_string(&absolute)?,
            }));
        }
        if let Some(text) = bundled(id) {
            return Ok(Some(Doc {
                source: Source::Bundled,
                text: text.to_owned(),
            }));
        }
        if let Some(name) = id.strip_prefix("clippy::") {
            return Ok(self.clippy_explain(name));
        }
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Ok(self.rustc_lint(id));
        }
        Ok(None)
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        command
    }

    fn clippy_explain(&self, name: &str) -> Option<Doc> {
        let output = self
            .command("cargo")
            .args(["clippy", "--explain", name])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (output.status.success() && !text.is_empty()).then_some(Doc {
            source: Source::ClippyExplain,
            text,
        })
    }

    fn rustc_lint(&self, name: &str) -> Option<Doc> {
        self.rustc_book(name).or_else(|| self.rustc_help(name))
    }

    fn rustc_book(&self, name: &str) -> Option<Doc> {
        let output = self
            .command("rustc")
            .args(["--print", "sysroot"])
            .output()
            .ok()?;
        let sysroot = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let listing = sysroot.join("share/doc/rust/html/rustc/lints/listing");
        let anchor = format!("id=\"{}\"", name.replace('_', "-"));
        for page in [
            "allowed-by-default.html",
            "warn-by-default.html",
            "deny-by-default.html",
        ] {
            let path = listing.join(page);
            let Ok(html) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(text) = html_section(&html, &anchor) {
                return Some(Doc {
                    source: Source::RustcBook(path),
                    text,
                });
            }
        }
        None
    }

    fn rustc_help(&self, name: &str) -> Option<Doc> {
        let output = self.command("rustc").args(["-W", "help"]).output().ok()?;
        let listing = String::from_utf8_lossy(&output.stdout);
        let wanted = name.replace('_', "-");
        listing.lines().find_map(|line| {
            let mut columns = line.split_whitespace();
            (columns.next() == Some(wanted.as_str())).then(|| Doc {
                source: Source::RustcHelp,
                text: format!(
                    "# {name}\n\n{}\n\nDefault level: {}. Full text: install the `rust-docs` component.",
                    columns.clone().skip(1).collect::<Vec<_>>().join(" "),
                    columns.next().unwrap_or("unknown")
                ),
            })
        })
    }
}

/// Text of the HTML section that starts at the heading carrying `anchor`, with tags stripped.
fn html_section(html: &str, anchor: &str) -> Option<String> {
    let start = html.find(anchor)?;
    let heading_start = html.get(..start)?.rfind('<')?;
    let rest = html.get(heading_start..)?;
    let end = rest.get(1..)?.find("<h2").map_or(rest.len(), |i| i + 1);
    let section = rest.get(..end)?;
    let mut text = String::new();
    let mut in_tag = false;
    for c in section.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    Some(lines.join("\n").trim().to_owned())
}
