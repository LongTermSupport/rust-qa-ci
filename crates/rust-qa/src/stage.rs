//! Project stages: commands a project declares in `qa/rust-qa.toml`, each run in one phase of the
//! gate, for what no built-in tool covers (another language's formatter, linter or tests, a lint
//! library's own suite, a check that a generated file is fresh).
//!
//! ```toml
//! [[stage]]
//! name = "shellcheck"                      # `-t` name and listed identifier
//! phase = "detectors"                      # format | detectors | runners
//! says = "ShellCheck over every shell script"
//! fix = "Quote expansions and check every status"
//! command = ["shellcheck", "{files}"]      # a whole run; `{files}` is one argument per file
//! files_command = ["shellcheck", "{files}"] # a subset run (`-p`); without it, `command` runs
//! files = ["**/*.sh"]                      # globs from the root choosing the files
//! shebangs = ["bash", "sh"]                # or files whose `#!` line names one of these
//! exclude = ["vendor/**"]
//! dir = "web"                              # working directory, with its own Rust toolchain
//! output = "findings"                      # status (the exit code) | findings (JSON on stdout)
//! rules = ["node", "scan.ts", "--rules"]   # prints the rules the stage reports, as JSON
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::hit::Hit;
use crate::manifest::workspace_files;
use crate::pipeline::Phase;
use crate::{Error, glob_set};

/// The argument that expands to the chosen files.
pub const FILES: &str = "{files}";

/// One project stage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    /// Name used with `-t`, and the identifier the stage is listed and documented under.
    pub name: String,
    /// Phase the stage runs in.
    pub phase: Phase,
    /// What the stage checks.
    pub says: String,
    /// How to put right what it reports.
    pub fix: String,
    /// The command for a whole run.
    pub command: Vec<String>,
    /// The command for a subset run, given the chosen files in the scope.
    #[serde(default)]
    pub files_command: Option<Vec<String>>,
    /// Globs, from the root, choosing the files `{files}` stands for.
    #[serde(default)]
    pub files: Vec<String>,
    /// Interpreters: a file whose `#!` line names one of these is chosen too.
    #[serde(default)]
    pub shebangs: Vec<String>,
    /// Globs, from the root, of files never chosen.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Working directory, relative to the root; the command runs with the Rust toolchain that
    /// directory's `rust-toolchain.toml` names, not the one `rust-qa` was started under.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// How the stage reports.
    #[serde(default)]
    pub output: Output,
    /// A command printing the rules the stage reports, as a JSON list of
    /// `{"id": ..., "description": ...}`, for the listing, the documentation check and the summary.
    #[serde(default)]
    pub rules: Option<Vec<String>>,
}

/// How a stage reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Output {
    /// The exit code decides; the command's own output is shown as it is.
    #[default]
    Status,
    /// The command prints a JSON list of `{"file", "line", "rule", "message", "note"?}` on stdout
    /// and exits 0 or 1; `rust-qa` applies the record and fails the gate on what remains.
    Findings,
}

/// A rule a stage reports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageRule {
    /// Identifier, as the stage prints it.
    pub id: String,
    /// What it finds.
    #[serde(default)]
    pub description: String,
}

impl Stage {
    /// What is wrong with the declaration, if anything.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let label = format!("stage `{}`", self.name);
        let mut problems = Vec::new();
        if self.name.trim().is_empty() {
            problems.push("a stage has no name".to_owned());
        }
        for (key, text) in [("says", &self.says), ("fix", &self.fix)] {
            if text.trim().is_empty() {
                problems.push(format!("{label}: `{key}` is empty"));
            }
        }
        let commands = std::iter::once(&self.command)
            .chain(self.files_command.as_ref())
            .chain(self.rules.as_ref());
        let mut uses_files = false;
        for command in commands {
            if command.is_empty() {
                problems.push(format!("{label}: empty command"));
            }
            for arg in command {
                if arg == FILES {
                    uses_files = true;
                } else if arg.contains(FILES) {
                    problems.push(format!(
                        "{label}: `{FILES}` must be a whole argument, not part of `{arg}`"
                    ));
                    uses_files = true;
                }
            }
        }
        if (uses_files || self.files_command.is_some()) && !self.selects_files() {
            problems.push(format!(
                "{label}: it selects no files; give `files` globs or `shebangs`"
            ));
        }
        for patterns in [&self.files, &self.exclude] {
            if let Err(error) = glob_set(patterns) {
                problems.push(format!("{label}: {error}"));
            }
        }
        problems
    }

    /// Whether the stage chooses files.
    #[must_use]
    pub fn selects_files(&self) -> bool {
        !self.files.is_empty() || !self.shebangs.is_empty()
    }

    /// Whether a command line of the stage takes the chosen files.
    #[must_use]
    pub fn takes_files(&self, scoped: bool) -> bool {
        self.chosen_command(scoped).iter().any(|arg| arg == FILES)
    }

    fn chosen_command(&self, scoped: bool) -> &[String] {
        match (&self.files_command, scoped) {
            (Some(command), true) => command,
            _ => &self.command,
        }
    }

    /// The files the stage is given, relative to the root, sorted: tracked or not, `.gitignore`
    /// honoured, `target` and `.git` skipped, limited to `scope` when it is not empty.
    ///
    /// # Errors
    /// Returns an error for an invalid glob.
    pub fn matching_files(&self, root: &Path, scope: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
        let chosen = glob_set(&self.files)?;
        let exclude = glob_set(&self.exclude)?;
        Ok(workspace_files(root, scope, &exclude, |rel| {
            chosen.is_match(rel) || self.interpreted(&root.join(rel))
        }))
    }

    fn interpreted(&self, path: &Path) -> bool {
        if self.shebangs.is_empty() {
            return false;
        }
        let Ok(text) = std::fs::read(path) else {
            return false;
        };
        let first = text.split(|b| *b == b'\n').next().unwrap_or(&[]);
        let line = String::from_utf8_lossy(first);
        let Some(rest) = line.strip_prefix("#!") else {
            return false;
        };
        let mut words = rest.split_whitespace();
        let program = words.next().map(basename).unwrap_or_default();
        let interpreter = if program == "env" {
            words.find(|w| !w.starts_with('-')).map(basename)
        } else {
            Some(program)
        };
        interpreter.is_some_and(|name| self.shebangs.iter().any(|s| s == name))
    }

    /// The command line for a run, with `{files}` expanded to `files` (relative to the root, and
    /// passed relative to the stage directory); `None` when the command takes files and there are
    /// none.
    #[must_use]
    pub fn argv(&self, scoped: bool, files: &[PathBuf]) -> Option<Vec<String>> {
        let command = self.chosen_command(scoped);
        let mut argv = Vec::new();
        for arg in command {
            if arg == FILES {
                if files.is_empty() {
                    return None;
                }
                argv.extend(files.iter().map(|file| self.relative_to_dir(file)));
            } else {
                argv.push(arg.clone());
            }
        }
        Some(argv)
    }

    fn relative_to_dir(&self, file: &Path) -> String {
        match &self.dir {
            Some(dir) => file.strip_prefix(dir).unwrap_or(file).display().to_string(),
            None => file.display().to_string(),
        }
    }

    /// The stage's documentation, for `rust-qa rule-doc <name>` when the project has no page.
    #[must_use]
    pub fn doc(&self) -> String {
        let mut text = format!(
            "# {}\n\nProject stage ({} phase): {}\n\n## How to fix correctly\n\n{}\n\n## What runs\n\n`{}`",
            self.name,
            self.phase.name(),
            self.says.trim(),
            self.fix.trim(),
            self.command.join(" ")
        );
        if let Some(dir) = &self.dir {
            let _ = write!(text, " in `{}`", dir.display());
        }
        text.push('\n');
        text
    }
}

fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFinding {
    file: String,
    line: usize,
    #[serde(default)]
    column: usize,
    rule: String,
    message: String,
    #[serde(default)]
    note: Option<String>,
}

/// Findings a stage printed, with paths made relative to the root (they are given relative to
/// the stage directory `dir`, or to the root without one).
///
/// # Errors
/// Returns the parser's message when the text is not a JSON list of findings.
pub fn parse_findings(json: &str, dir: Option<&Path>) -> Result<Vec<Hit>, String> {
    let raw: Vec<RawFinding> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(|finding| Hit {
            path: dir.map_or_else(
                || PathBuf::from(&finding.file),
                |dir| dir.join(&finding.file),
            ),
            rule: finding.rule,
            line: finding.line,
            column: finding.column,
            message: finding.message,
            note: finding.note,
            rendered: None,
            blocking: true,
        })
        .collect())
}

/// The rules a stage's `rules` command printed.
///
/// # Errors
/// Returns the parser's message when the text is not a JSON list of rules.
pub fn parse_rules(json: &str) -> Result<Vec<StageRule>, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}
