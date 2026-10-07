# rqaci.ast.shell-c-command

Level: error. Engine: ast-grep (bundled pack, on by default).

## What it flags

An `.arg(..)` or `.args(..)` call that passes a shell's `-c` flag (alone or combined, such as
`-lc` or `-ec`) on a builder chain rooted in `Command::new("<shell>")`, where the shell is one of
`sh`, `bash`, `zsh`, `dash`, `ksh`, `mksh`, `ash`, `fish`. The path before `Command::new` does
not matter, so `std::process::Command` and async equivalents are covered:

```rust
Command::new("sh").arg("-c").arg(format!("tar -xf {archive}"));
std::process::Command::new("bash").args(["-c", &script]);
```

Not flagged: a shell running a script file (`Command::new("sh").arg("build.sh")`), `-c` passed to
any other program, and a command assembled across several statements
(`let mut c = Command::new("sh"); c.arg("-c");`), which this single-expression rule cannot see.

## Why

`-c` hands a string to a shell for parsing. Anything interpolated into that string (a file name,
a branch, user input) is parsed as shell syntax: spaces split it, quotes and `;`, `$(..)`,
backticks and globs are interpreted. That is a command-injection hole and, even with trusted
input, a quoting bug waiting for the first file name with a space. It also makes the program
depend on which shell is installed and how it is configured.

## How to fix correctly

Run the program directly and pass each argument as its own `arg`; the operating system receives
the exact argument vector and nothing is reparsed:

```rust
use std::process::Command;

let status = Command::new("tar").arg("-xf").arg(&archive).status()?;
```

For a pipeline, connect the processes in Rust (`Stdio::piped()` and the child's `stdout`) or do
the work in Rust. If a shell script really is needed, ship it as a file, run
`Command::new("sh").arg("path/to/script.sh")`, and pass values as further arguments (read as
`"$1"` inside the script), never by splicing them into the script text.

## Why syntax suffices

The rule fires only on what the source spells out literally: the program name is a string literal
passed to `Command::new`, and `-c` is a string literal passed to `arg`/`args` in the same method
chain. Both facts are in the syntax tree of one expression; no type, trait or cross-file
information changes the verdict. Name resolution is not needed because no other common builder
pairs a `Command::new("sh")` constructor with an `arg("-c")` call. Commands built across
statements or from variables are not flagged (false negatives, never false positives).

No Clippy or rustc lint inspects process arguments, so this does not duplicate one.

## Suppression

Only with a project-record exception. Put `// ast-grep-ignore: rqaci.ast.shell-c-command` on the
line above the call, and record it:

```toml
[[exception]]
rule = "ast-grep::rqaci.ast.shell-c-command"
path = "src/tasks/maintenance.rs"
justification = "Runs a fixed, literal maintenance one-liner with no interpolated input."
```
