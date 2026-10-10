//! Offline identifier-to-documentation resolution.
//!
//! Order: project document (`<docs_dir>/<id>.md`, else a page whose name is the identifier as a
//! lint name, without case and with `-` for `_`), a project stage's own description, bundled
//! document, the installed Clippy (`cargo clippy --explain`), then the installed rustc (lint listing
//! in the `rust-docs` component, else the one-line description from `rustc -W help`). Nothing is
//! fetched from the network.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;
use crate::config::DocResolver;
use crate::record::lint_key;
use crate::stage::Stage;

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
        "rqaci.docs.dangling",
        include_str!("../docs/rules/rqaci.docs.dangling.md"),
    ),
    (
        "rqaci.docs.orphan",
        include_str!("../docs/rules/rqaci.docs.orphan.md"),
    ),
    (
        "rqaci.rules.untested",
        include_str!("../docs/rules/rqaci.rules.untested.md"),
    ),
    (
        "rqaci.record.budget",
        include_str!("../docs/rules/rqaci.record.budget.md"),
    ),
    (
        "rqaci.record.expired",
        include_str!("../docs/rules/rqaci.record.expired.md"),
    ),
    (
        "rqaci.record.invalid",
        include_str!("../docs/rules/rqaci.record.invalid.md"),
    ),
    (
        "rqaci.record.ratchet",
        include_str!("../docs/rules/rqaci.record.ratchet.md"),
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
        "rqaci.suppression.allow",
        include_str!("../docs/rules/rqaci.suppression.allow.md"),
    ),
    (
        "rqaci.suppression.unrecorded",
        include_str!("../docs/rules/rqaci.suppression.unrecorded.md"),
    ),
    (
        "rqaci_sensitive_debug_derive",
        include_str!("../docs/rules/rqaci_sensitive_debug_derive.md"),
    ),
    (
        "rqaci_suppression_generated",
        include_str!("../docs/rules/rqaci_suppression_generated.md"),
    ),
    ("warnings", include_str!("../docs/rules/warnings.md")),
    (
        "rqaci.ast.hardcoded-temp-path",
        include_str!("../docs/rules/rqaci.ast.hardcoded-temp-path.md"),
    ),
    (
        "rqaci.ast.shell-c-command",
        include_str!("../docs/rules/rqaci.ast.shell-c-command.md"),
    ),
    (
        "rqaci.ast.tls-verification-disabled",
        include_str!("../docs/rules/rqaci.ast.tls-verification-disabled.md"),
    ),
];

/// Prefix of ast-grep rule ids in the project record (see the suppression guard).
const AST_GREP_PREFIX: &str = "ast-grep::";

/// Identifiers that must resolve (every listed defence plus the built-ins), sorted, and those that
/// do not.
///
/// # Errors
/// Returns an error when defences cannot be enumerated or a project document cannot be read.
pub fn dangling(
    root: &Path,
    config: &crate::config::Config,
    dylint: &dyn Fn(&Path) -> Result<Vec<crate::defences::DylintLint>, Error>,
) -> Result<(Vec<String>, Vec<String>), Error> {
    let resolver = Resolver::for_config(root, config);
    let mut ids: Vec<String> = crate::defences::enumerate(root, config, dylint)?
        .into_iter()
        .map(|d| d.id)
        .collect();
    ids.extend(crate::builtin::ALL.iter().map(|id| (*id).to_owned()));
    ids.sort();
    ids.dedup();
    let mut missing = Vec::new();
    for id in &ids {
        if resolver.resolve(id)?.is_none() {
            missing.push(id.clone());
        }
    }
    Ok((ids, missing))
}

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

/// The first paragraph of the `## <heading>` section of a markdown page, for the first of
/// `headings` the page has, on one line.
#[must_use]
pub fn section_paragraph(text: &str, headings: &[String]) -> Option<String> {
    for heading in headings {
        let marker = format!("## {heading}");
        let mut lines = text.lines().skip_while(|line| line.trim_end() != marker);
        if lines.next().is_none() {
            continue;
        }
        let paragraph: Vec<&str> = lines
            .skip_while(|line| line.trim().is_empty())
            .take_while(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .map(str::trim)
            .collect();
        if !paragraph.is_empty() {
            let text = paragraph.join(" ");
            // A paragraph that introduces an example ends where the example would begin.
            return Some(match text.strip_suffix(':') {
                Some(lead) => format!("{lead}."),
                None => text,
            });
        }
    }
    None
}

/// Whether the page named `stem` (its file name without `.md`) documents `id`.
fn documents(stem: &str, id: &str) -> bool {
    let named = id.replace("::", ".");
    stem == named || lint_key(stem) == lint_key(&named)
}

/// Pages in `docs_dir` (relative to `root`) that document none of `ids`, relative to the root and
/// sorted. `README.md` is never an orphan; a missing directory holds none.
///
/// # Errors
/// Returns an error when the directory exists but cannot be read.
pub fn orphans(root: &Path, docs_dir: &Path, ids: &[String]) -> Result<Vec<PathBuf>, Error> {
    let dir = root.join(docs_dir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Read { path: dir, source }),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| Error::Read {
                path: dir.clone(),
                source,
            })?
            .path();
        if path.extension().is_none_or(|e| e != "md") || !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if stem != "README" && !ids.iter().any(|id| documents(&stem, id)) {
            found.push(docs_dir.join(format!("{stem}.md")));
        }
    }
    found.sort();
    Ok(found)
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
    /// A project stage's own `says` and `fix`, or the documentation its `rules` command gives.
    Stage,
    /// A `[[resolver]]` text from the configuration.
    Configured,
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
    stages: Vec<Stage>,
    resolvers: Vec<DocResolver>,
    /// The rules every stage's `rules` command printed, read once.
    stage_rules: std::cell::OnceCell<Vec<crate::stage::StageRule>>,
}

impl Resolver {
    /// Resolver for the workspace at `root` with project documents in `docs_dir` (relative to root).
    #[must_use]
    pub fn new(root: &Path, docs_dir: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            docs_dir: docs_dir.to_path_buf(),
            stages: Vec::new(),
            resolvers: Vec::new(),
            stage_rules: std::cell::OnceCell::new(),
        }
    }

    /// Resolver for the workspace at `root` as `config` describes it, its project stages included.
    #[must_use]
    pub fn for_config(root: &Path, config: &crate::config::Config) -> Self {
        Self {
            stages: config.stages.clone(),
            resolvers: config.resolvers.clone(),
            ..Self::new(root, &config.docs_dir)
        }
    }

    /// The project page for `id`, under its own name or another spelling of it, if one exists.
    ///
    /// # Errors
    /// Returns an error when the page exists but cannot be read.
    pub fn project_page(&self, id: &str) -> Result<Option<String>, Error> {
        let id = id.trim();
        let id = id.strip_prefix(AST_GREP_PREFIX).unwrap_or(id);
        let exact = self.docs_dir.join(doc_file_name(id));
        let page = if self.root.join(&exact).is_file() {
            Some(exact)
        } else {
            self.project_alias(id)
        };
        page.map(|page| crate::read_to_string(&self.root.join(page)))
            .transpose()
    }

    /// The project page documenting `id` under another spelling of its name, if one exists.
    fn project_alias(&self, id: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(self.root.join(&self.docs_dir)).ok()?;
        let mut pages: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "md") && path.is_file())
            .filter(|path| {
                path.file_stem()
                    .is_some_and(|stem| documents(&stem.to_string_lossy(), id))
            })
            .collect();
        pages.sort();
        pages
            .into_iter()
            .next()
            .and_then(|path| path.file_name().map(|name| self.docs_dir.join(name)))
    }

    /// Resolves `id`; `Ok(None)` means the identifier is dangling.
    ///
    /// # Errors
    /// Returns an error when a project document exists but cannot be read.
    pub fn resolve(&self, id: &str) -> Result<Option<Doc>, Error> {
        let id = id.trim();
        // Suppressions of ast-grep rules are recorded as `ast-grep::<rule id>`; the document
        // belongs to the rule id itself.
        let id = id.strip_prefix(AST_GREP_PREFIX).unwrap_or(id);
        let project = self.docs_dir.join(doc_file_name(id));
        let absolute = self.root.join(&project);
        if absolute.is_file() {
            return Ok(Some(Doc {
                source: Source::Project(project),
                text: crate::read_to_string(&absolute)?,
            }));
        }
        if let Some(alias) = self.project_alias(id) {
            return Ok(Some(Doc {
                text: crate::read_to_string(&self.root.join(&alias))?,
                source: Source::Project(alias),
            }));
        }
        if let Some(stage) = self.stages.iter().find(|stage| stage.name == id) {
            return Ok(Some(Doc {
                source: Source::Stage,
                text: stage.doc(),
            }));
        }
        if let Some(text) = self.stage_rule_doc(id)? {
            return Ok(Some(Doc {
                source: Source::Stage,
                text,
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
        if !id.is_empty()
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && let Some(doc) = self.rustc_lint(id)
        {
            return Ok(Some(doc));
        }
        Ok(self.configured(id))
    }

    /// The documentation a stage's `rules` command gives for one of its rules.
    fn stage_rule_doc(&self, id: &str) -> Result<Option<String>, Error> {
        if self.stage_rules.get().is_none() {
            let mut all = Vec::new();
            for stage in self.stages.iter().filter(|stage| stage.rules.is_some()) {
                all.extend(crate::defences::stage_rules(&self.root, stage)?);
            }
            let _ = self.stage_rules.set(all);
        }
        Ok(self
            .stage_rules
            .get()
            .and_then(|rules| rules.iter().find(|rule| rule.id == id))
            .and_then(|rule| rule.doc.clone()))
    }

    /// The text of the first configured resolver whose glob matches `id`.
    fn configured(&self, id: &str) -> Option<Doc> {
        self.resolvers.iter().find_map(|resolver| {
            let glob = globset::Glob::new(&resolver.matches)
                .ok()?
                .compile_matcher();
            glob.is_match(id).then(|| Doc {
                source: Source::Configured,
                text: resolver.text.replace("{id}", id),
            })
        })
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
