//! `rust-qa`: one QA entry point for a Rust workspace.
//!
//! Phases run in a fixed order (format, detectors, runners); a detector failure stops the run before
//! any runner starts. The crate also lists active defences, resolves rule identifiers to offline
//! documentation, validates the project record of exceptions and forbids unrecorded suppressions.

pub mod ast_grep;
pub mod builtin;
pub mod config;
pub mod date;
pub mod defences;
pub mod diagnostics;
pub mod docs;
pub mod finding;
pub mod hit;
pub mod manifest;
pub mod pipeline;
pub mod policy;
pub mod record;
pub mod stage;
pub mod summary;
pub mod suppression;
pub mod tools;

/// Failure line printed whenever a defence fails, pointing at the method the gate implements.
pub const DBF_POINTER: &str = "Defence Before Fix: each failure names a rule id; run `rust-qa rule-doc <id>` \
     for the correct construction. Method: https://defence-before-fix.github.io/";

/// Errors raised while loading configuration or inspecting the workspace.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// File that failed.
        path: std::path::PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// A file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// File that failed.
        path: std::path::PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// A TOML file did not parse or did not match the expected schema.
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        /// File that failed.
        path: std::path::PathBuf,
        /// Parser error.
        source: Box<toml::de::Error>,
    },
    /// A YAML file did not parse.
    #[error("invalid YAML in {path}: {message}")]
    Yaml {
        /// File that failed.
        path: std::path::PathBuf,
        /// Parser message.
        message: String,
    },
    /// A glob pattern was invalid.
    #[error("invalid glob `{pattern}`: {source}")]
    Glob {
        /// The pattern as written.
        pattern: String,
        /// Parser error.
        source: globset::Error,
    },
    /// An external command could not be started or returned unusable output.
    #[error("command `{command}` failed: {message}")]
    Command {
        /// Command line, for the operator.
        command: String,
        /// What went wrong.
        message: String,
    },
    /// The command line asked for something that does not exist.
    #[error("{0}")]
    Usage(String),
}

/// Reads a file to a string, mapping the error to [`Error::Read`].
///
/// # Errors
/// Returns [`Error::Read`] when the file cannot be read.
pub fn read_to_string(path: &std::path::Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Parses a TOML file into `T`, mapping errors to [`Error::Toml`].
///
/// # Errors
/// Returns [`Error::Read`] or [`Error::Toml`].
pub fn read_toml<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, Error> {
    let text = read_to_string(path)?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

/// Builds a glob matcher from patterns, mapping errors to [`Error::Glob`].
///
/// # Errors
/// Returns [`Error::Glob`] for the first invalid pattern.
pub fn glob_set<S: AsRef<str>>(patterns: &[S]) -> Result<globset::GlobSet, Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let pattern = pattern.as_ref();
        let glob = globset::Glob::new(pattern).map_err(|source| Error::Glob {
            pattern: pattern.to_owned(),
            source,
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|source| Error::Glob {
        pattern: String::new(),
        source,
    })
}
