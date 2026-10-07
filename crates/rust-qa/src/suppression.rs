//! Suppression guard (`rqaci.suppression.unrecorded`).
//!
//! Finds every route that silences a defence and requires a matching project-record exception:
//! lint attributes (`allow`, `expect`, including inside `cfg_attr` and macro bodies), ast-grep ignore
//! comments, `allow` levels in Cargo `[lints]` tables, `-A`/`--allow`/`--cap-lints` in
//! `.cargo/config.toml` rustflags, `allow-*` switches in `clippy.toml`, and cargo-deny advisory
//! ignores. Attributes are matched on the parsed syntax tree because they are syntax by definition;
//! no type information can add or remove a suppression.

use std::path::{Path, PathBuf};

use globset::GlobSet;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Meta, Token};
use toml::Value;

use crate::builtin::{RECORD_STALE, SUPPRESSION_ALLOW, SUPPRESSION_UNRECORDED};
use crate::finding::Finding;
use crate::manifest::{Level, lint_settings, workspace_files};
use crate::record::{Record, normalise_rule};
use crate::{Error, read_to_string};

/// How a suppression is expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `#[expect]` attribute, possibly inside `cfg_attr`.
    Attribute,
    /// `#[allow]` attribute, possibly inside `cfg_attr`; also fails `rqaci.suppression.allow`.
    AllowAttribute,
    /// Lint attribute written inside a macro invocation or definition.
    MacroAttribute,
    /// `// ast-grep-ignore` comment.
    AstGrepIgnore,
    /// `allow` level in a Cargo `[lints]` table.
    ManifestLints,
    /// `-A`, `--allow` or `--cap-lints` in `.cargo/config.toml` rustflags.
    Rustflags,
    /// `allow-*` switch in `clippy.toml`.
    ClippyConfig,
    /// `[advisories] ignore` entry in `deny.toml`.
    DenyIgnore,
}

impl Route {
    /// Short human description.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Attribute => "lint attribute",
            Self::AllowAttribute => "allow attribute",
            Self::MacroAttribute => "lint attribute in a macro",
            Self::AstGrepIgnore => "ast-grep ignore comment",
            Self::ManifestLints => "Cargo [lints] allow level",
            Self::Rustflags => "rustflags lint override",
            Self::ClippyConfig => "clippy.toml allow switch",
            Self::DenyIgnore => "cargo-deny advisory ignore",
        }
    }
}

/// One suppression found in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// Rule identifier being silenced, as the defence prints it.
    pub rule: String,
    /// File relative to the root.
    pub path: PathBuf,
    /// One-based line.
    pub line: usize,
    /// How it is expressed.
    pub route: Route,
}

/// Result of a scan: suppressions found and Rust files that could not be parsed.
#[derive(Debug, Default)]
pub struct ScanResult {
    /// Suppressions in path, then line, order.
    pub suppressions: Vec<Suppression>,
    /// Files whose suppressions could not be determined, with the parser message.
    pub unparseable: Vec<(PathBuf, String)>,
}

/// Scans the workspace (or the `scope` subset) for suppressions.
///
/// # Errors
/// Returns an error when a configuration file cannot be read or parsed.
pub fn scan(root: &Path, scope: &[PathBuf], exclude: &GlobSet) -> Result<ScanResult, Error> {
    let files = workspace_files(root, scope, exclude, is_scanned);
    let mut result = ScanResult::default();
    for rel in files {
        let text = read_to_string(&root.join(&rel))?;
        let mut found: Vec<(String, usize, Route)> = Vec::new();
        let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "Cargo.toml" {
            for setting in lint_settings(&text, &rel)? {
                if setting.level == Level::Allow {
                    found.push((setting.id, setting.line, Route::ManifestLints));
                }
            }
        } else if name == "clippy.toml" || name == ".clippy.toml" {
            clippy_config(&text, &rel, &mut found)?;
        } else if name == "deny.toml" {
            deny_ignores(&text, &rel, &mut found)?;
        } else if is_cargo_config(&rel) {
            rustflags(&text, &rel, &mut found)?;
        } else {
            match syn::parse_file(&text) {
                Ok(file) => {
                    let mut visitor = AttrVisitor { out: &mut found };
                    visitor.visit_file(&file);
                }
                Err(error) => result.unparseable.push((rel.clone(), error.to_string())),
            }
            ast_grep_ignores(&text, &mut found);
        }
        found.sort_by_key(|(_, line, _)| *line);
        result
            .suppressions
            .extend(found.into_iter().map(|(rule, line, route)| Suppression {
                rule,
                path: rel.clone(),
                line,
                route,
            }));
    }
    Ok(result)
}

/// Turns a scan into findings: unrecorded suppressions, unparseable files, and (on full scans)
/// record exceptions that match nothing.
#[must_use]
pub fn check(
    result: &ScanResult,
    record: &Record,
    record_path: &Path,
    report_stale: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (path, message) in &result.unparseable {
        findings.push(Finding {
            rule: SUPPRESSION_UNRECORDED,
            path: path.clone(),
            line: 0,
            message: format!("cannot parse ({message}); its suppressions cannot be verified"),
        });
    }
    for suppression in &result.suppressions {
        if suppression.route == Route::AllowAttribute
            && record.find(SUPPRESSION_ALLOW, &suppression.path).is_none()
        {
            findings.push(Finding {
                rule: SUPPRESSION_ALLOW,
                path: suppression.path.clone(),
                line: suppression.line,
                message: format!(
                    "`allow` silences `{}` with no end date; use `expect({}, reason = \"...\")`, which fails once the lint stops firing",
                    suppression.rule, suppression.rule
                ),
            });
        }
        if record.find(&suppression.rule, &suppression.path).is_none() {
            findings.push(Finding {
                rule: SUPPRESSION_UNRECORDED,
                path: suppression.path.clone(),
                line: suppression.line,
                message: format!(
                    "{} silences `{}` with no exception in {}",
                    suppression.route.describe(),
                    suppression.rule,
                    record_path.display()
                ),
            });
        }
    }
    if report_stale {
        for index in stale(result, record) {
            let Some(exception) = record.exceptions.get(index) else {
                continue;
            };
            findings.push(Finding {
                rule: RECORD_STALE,
                path: record_path.to_path_buf(),
                line: 0,
                message: format!(
                    "exception #{} (rule `{}`, path `{}`) matches no suppression; remove it (`rust-qa record prune`)",
                    index + 1,
                    exception.rule,
                    exception.path
                ),
            });
        }
    }
    findings
}

/// Indexes of record exceptions that cover no suppression in a full scan.
#[must_use]
pub fn stale(result: &ScanResult, record: &Record) -> Vec<usize> {
    let mut used = vec![false; record.exceptions.len()];
    for suppression in &result.suppressions {
        let mut rules = vec![suppression.rule.as_str()];
        if suppression.route == Route::AllowAttribute {
            rules.push(SUPPRESSION_ALLOW);
        }
        for rule in rules {
            if let Some(index) = record.find(rule, &suppression.path)
                && let Some(slot) = used.get_mut(index)
            {
                *slot = true;
            }
        }
    }
    (0..used.len())
        .filter(|index| used.get(*index) == Some(&false))
        .collect()
}

fn is_scanned(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    rel.extension().is_some_and(|e| e == "rs")
        || matches!(
            name,
            "Cargo.toml" | "clippy.toml" | ".clippy.toml" | "deny.toml"
        )
        || is_cargo_config(rel)
}

fn is_cargo_config(rel: &Path) -> bool {
    let in_cargo_dir = rel
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|dir| dir == ".cargo");
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    in_cargo_dir && matches!(name, "config.toml" | "config")
}

struct AttrVisitor<'a> {
    out: &'a mut Vec<(String, usize, Route)>,
}

impl<'ast> Visit<'ast> for AttrVisitor<'_> {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        collect_meta(
            &attr.meta,
            attr.span().start().line,
            Route::Attribute,
            self.out,
        );
        syn::visit::visit_attribute(self, attr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        scan_tokens(mac.tokens.clone(), self.out);
        syn::visit::visit_macro(self, mac);
    }
}

fn collect_meta(meta: &Meta, line: usize, route: Route, out: &mut Vec<(String, usize, Route)>) {
    let Meta::List(list) = meta else {
        return;
    };
    let Some(name) = list.path.get_ident().map(ToString::to_string) else {
        return;
    };
    let Ok(nested) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return;
    };
    match name.as_str() {
        "allow" | "expect" => {
            let route = if name == "allow" && route == Route::Attribute {
                Route::AllowAttribute
            } else {
                route
            };
            for item in nested {
                if let Meta::Path(path) = item {
                    let segments: Vec<String> =
                        path.segments.iter().map(|s| s.ident.to_string()).collect();
                    out.push((normalise_rule(&segments.join("::")), line, route));
                }
            }
        }
        "cfg_attr" => {
            for item in nested.iter().skip(1) {
                collect_meta(item, line, route, out);
            }
        }
        _ => {}
    }
}

fn scan_tokens(stream: TokenStream, out: &mut Vec<(String, usize, Route)>) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Punct(punct) = token
            && punct.as_char() == '#'
        {
            let mut next = index + 1;
            if let Some(TokenTree::Punct(bang)) = tokens.get(next)
                && bang.as_char() == '!'
            {
                next += 1;
            }
            if let Some(TokenTree::Group(group)) = tokens.get(next)
                && group.delimiter() == Delimiter::Bracket
                && let Ok(meta) = syn::parse2::<Meta>(group.stream())
            {
                collect_meta(&meta, group.span().start().line, Route::MacroAttribute, out);
            }
        }
        if let TokenTree::Group(group) = token
            && group.delimiter() != Delimiter::Bracket
        {
            scan_tokens(group.stream(), out);
        }
    }
}

fn ast_grep_ignores(text: &str, out: &mut Vec<(String, usize, Route)>) {
    const MARKER: &str = "ast-grep-ignore";
    for (line, comment) in comments(text) {
        let is_doc = ["///", "//!", "/*!"].iter().any(|p| comment.starts_with(p))
            || (comment.starts_with("/**") && !comment.starts_with("/**/"));
        if is_doc {
            continue;
        }
        let body = comment
            .strip_prefix("//")
            .or_else(|| comment.strip_prefix("/*"))
            .unwrap_or(&comment)
            .trim_start();
        let Some(rest) = body.strip_prefix(MARKER) else {
            continue;
        };
        let rest = rest.trim_end_matches("*/");
        let ids: Vec<&str> = rest
            .trim_start()
            .strip_prefix(':')
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            out.push(("ast-grep::*".to_owned(), line, Route::AstGrepIgnore));
        }
        for id in ids {
            let id = id.split_whitespace().next().unwrap_or(id);
            out.push((format!("ast-grep::{id}"), line, Route::AstGrepIgnore));
        }
    }
}

/// Comments in Rust source with their starting line, skipping string and character literals.
fn comments(text: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    while i < chars.len() {
        let c = at(i);
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            let start = i;
            while i < chars.len() && at(i) != '\n' {
                i += 1;
            }
            found.push((line, chars.get(start..i).unwrap_or(&[]).iter().collect()));
        } else if c == '/' && at(i + 1) == '*' {
            let (start, start_line) = (i, line);
            let mut depth = 0usize;
            while i < chars.len() {
                if at(i) == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if at(i) == '*' && at(i + 1) == '/' {
                    depth = depth.saturating_sub(1);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if at(i) == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
            found.push((
                start_line,
                chars.get(start..i).unwrap_or(&[]).iter().collect(),
            ));
        } else if c == 'r' && starts_literal(&chars, i) {
            let mut hashes = 0;
            let mut j = i + 1;
            while at(j) == '#' {
                hashes += 1;
                j += 1;
            }
            if at(j) == '"' {
                j += 1;
                loop {
                    if j >= chars.len() {
                        break;
                    }
                    if at(j) == '\n' {
                        line += 1;
                    }
                    if at(j) == '"' && (1..=hashes).all(|k| at(j + k) == '#') {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                i = j;
            } else {
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            while i < chars.len() && at(i) != '"' {
                if at(i) == '\\' {
                    i += 1;
                }
                if at(i) == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 1;
        } else if c == '\'' {
            if at(i + 1) == '\\' {
                i += 2;
                while i < chars.len() && at(i) != '\'' {
                    i += 1;
                }
                i += 1;
            } else if at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    found
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether the `r` at `i` starts a raw string literal (`r"`, `r#"`, `br"`) rather than ending an
/// identifier.
fn starts_literal(chars: &[char], i: usize) -> bool {
    let before = |back: usize| i.checked_sub(back).and_then(|k| chars.get(k)).copied();
    let free = |c: Option<char>| c.is_none_or(|c| !is_ident_char(c));
    free(before(1)) || (before(1) == Some('b') && free(before(2)))
}

/// Line of `key` inside the table whose header is `header` (`None` for the root table).
fn line_in_table(text: &str, header: Option<&str>, key: &str) -> usize {
    let mut current: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            current = Some(
                trimmed
                    .trim_matches(|c| c == '[' || c == ']')
                    .trim()
                    .to_owned(),
            );
            continue;
        }
        let this_key = trimmed
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        if current.as_deref() == header && this_key == key {
            return index + 1;
        }
    }
    0
}

fn parse_toml(text: &str, rel: &Path) -> Result<Value, Error> {
    toml::from_str(text).map_err(|source| Error::Toml {
        path: rel.to_path_buf(),
        source: Box::new(source),
    })
}

fn clippy_config(
    text: &str,
    rel: &Path,
    out: &mut Vec<(String, usize, Route)>,
) -> Result<(), Error> {
    let value = parse_toml(text, rel)?;
    let Some(table) = value.as_table() else {
        return Ok(());
    };
    for (key, setting) in table {
        if key.starts_with("allow-") && setting.as_bool() == Some(true) {
            out.push((
                format!("clippy.toml::{key}"),
                line_in_table(text, None, key),
                Route::ClippyConfig,
            ));
        }
    }
    Ok(())
}

fn deny_ignores(
    text: &str,
    rel: &Path,
    out: &mut Vec<(String, usize, Route)>,
) -> Result<(), Error> {
    let value = parse_toml(text, rel)?;
    let Some(ignores) = value
        .get("advisories")
        .and_then(|a| a.get("ignore"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    let line = line_in_table(text, Some("advisories"), "ignore");
    for entry in ignores {
        let id = match entry {
            Value::String(id) => Some(id.as_str()),
            Value::Table(table) => table.get("id").and_then(Value::as_str),
            _ => None,
        };
        if let Some(id) = id {
            out.push((format!("cargo-deny::{id}"), line, Route::DenyIgnore));
        }
    }
    Ok(())
}

fn rustflags(text: &str, rel: &Path, out: &mut Vec<(String, usize, Route)>) -> Result<(), Error> {
    let value = parse_toml(text, rel)?;
    let mut sources: Vec<(String, &Value)> = Vec::new();
    if let Some(flags) = value.get("build").and_then(|b| b.get("rustflags")) {
        sources.push(("build".to_owned(), flags));
    }
    if let Some(targets) = value.get("target").and_then(Value::as_table) {
        for (name, target) in targets {
            if let Some(flags) = target.get("rustflags") {
                let header = if name.contains(['.', '(', ' ', '\'']) {
                    format!("target.'{name}'")
                } else {
                    format!("target.{name}")
                };
                sources.push((header, flags));
            }
        }
    }
    for (header, flags) in sources {
        let line = line_in_table(text, Some(&header), "rustflags");
        let tokens: Vec<String> = match flags {
            Value::String(text) => text.split_whitespace().map(ToOwned::to_owned).collect(),
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        for rule in lowered_lints(&tokens) {
            out.push((rule, line, Route::Rustflags));
        }
    }
    Ok(())
}

/// Lints silenced by a rustc flag list.
fn lowered_lints(tokens: &[String]) -> Vec<String> {
    let mut rules = Vec::new();
    let mut iter = tokens.iter().peekable();
    while let Some(token) = iter.next() {
        if token == "--cap-lints" || token.starts_with("--cap-lints=") {
            rules.push("rustflags::cap-lints".to_owned());
            if token == "--cap-lints" {
                iter.next();
            }
        } else if token == "-A" || token == "--allow" {
            if let Some(lint) = iter.next() {
                rules.push(normalise_rule(lint));
            }
        } else if let Some(lint) = token.strip_prefix("--allow=") {
            rules.push(normalise_rule(lint));
        } else if let Some(lint) = token.strip_prefix("-A")
            && !lint.is_empty()
        {
            rules.push(normalise_rule(lint));
        }
    }
    rules
}
