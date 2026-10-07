# rqaci.ast.hardcoded-temp-path

Level: error. Engine: ast-grep (bundled pack, on by default).

## What it flags

A string literal or raw string literal whose value is `/tmp` or `/var/tmp`, or starts with
`/tmp/` or `/var/tmp/`:

```rust
std::fs::write("/tmp/report.txt", data)?;
let cache = Path::new("/tmp").join("cache");
```

Not flagged: other paths that merely contain `tmp` (`"/tmpfs"`, `"tmp/relative"`, `"/srv/tmp"`),
comments, and paths built at run time.

## Why

`/tmp` is shared by every user and process on the machine. A fixed name there is predictable, so
another process can create it first (often as a symlink to a file you did not mean to touch), read
what you write, or race you between check and use. Parallel tests and concurrent runs collide on
the same name and become flaky. The path is also wrong on platforms and containers where the
temporary directory lives elsewhere (`TMPDIR`, Windows).

## How to fix correctly

Create a private, uniquely named file or directory and let it clean itself up:

```rust
let dir = tempfile::tempdir()?;           // removed when `dir` is dropped
let report = dir.path().join("report.txt");
std::fs::write(&report, data)?;

let mut file = tempfile::NamedTempFile::new()?; // a single temporary file
```

When a file must outlive the process, choose a real location (a configured data or cache
directory), not the shared temporary directory. Where only the platform's temporary directory is
needed as a base, use `std::env::temp_dir()` and still create a unique entry inside it with
`tempfile`.

## Why syntax suffices

The finding is the literal value of one string token: the rule compares the text of each string
and raw string literal with a fixed prefix. Nothing outside that token (types, other files, the
call it is passed to) can make a literal `/tmp/...` path less predictable. Paths assembled at run
time are not flagged (false negatives, never false positives).

No Clippy or rustc lint inspects path literals, so this does not duplicate one.

## Suppression

Only with a project-record exception. Put `// ast-grep-ignore: rqaci.ast.hardcoded-temp-path` on
the line above the literal, and record it:

```toml
[[exception]]
rule = "ast-grep::rqaci.ast.hardcoded-temp-path"
path = "src/sandbox/mounts.rs"
justification = "Declares the mount point inside an isolated sandbox image; nothing writes there on the host."
```
