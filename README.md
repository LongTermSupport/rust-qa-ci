# rust-qa-ci

One QA entry point for Rust workspaces, built for the Defence Before Fix method
(https://defence-before-fix.github.io/): bespoke rules on rustc's type information, blocking
enforcement through a single command, offline rule documentation, a project record for every
exception, and an enumerable list of active defences.

Status: early (0.x). Conformance is **partial**; see [docs/CONFORMANCE.md](docs/CONFORMANCE.md).
Licence: not yet chosen; until it is, no licence is granted.

## What it runs

`rust-qa run` executes three phases in a fixed order. Every detector runs so one invocation reports
all static findings; any format or detector failure stops the run before the first runner.

| Phase | Tool (`-t` name) | What |
|---|---|---|
| format | `fmt` | `cargo fmt` (fixes locally; `--no-fix` checks) |
| detectors | `record` | project record validation (`rqaci.record.invalid`) |
| | `suppression` | unrecorded suppressions (`rqaci.suppression.unrecorded`, `rqaci.record.stale`) |
| | `summary` | agent summary freshness (`rqaci.summary.stale`) |
| | `docs` | every listed identifier resolves to documentation (`rqaci.docs.dangling`) |
| | `ast-grep` | supplementary syntactic rules: the project's `sgconfig.yml` rules, then the bundled `rqaci.ast.*` pack |
| | `clippy` | Clippy with `-D warnings` and the Cargo `[lints]` tables |
| | `dylint` | type-aware rule libraries from `[workspace.metadata.dylint]` |
| runners | `deny` | cargo-deny (advisories, bans, licences, sources) |
| | `audit` | cargo-audit (off by default) |
| | `machete` | unused dependencies |
| | `test` | cargo-nextest, then doctests |
| | `coverage` | cargo-llvm-cov with a line threshold |
| | `mutants` | cargo-mutants (`off`, `diff` against a base, or `full`) |

Options: `-t <tool>` (repeatable), `-p <path>` (repeatable; one file is enough; per-crate engines
analyse the owning crate and print only diagnostics inside the path), `--ci` (also implied by a
non-empty `CI` variable: autofix, then fail if the working tree changed), `--no-fix`, `--fail-fast`.

Other commands, none of which run a defence:

- `rust-qa rules [--json]`: every active defence, derived from configuration, plus the project record
- `rust-qa rule-doc <id>`: documentation for any printed identifier, offline
- `rust-qa docs-check`: fail if a listed identifier has no documentation
- `rust-qa record check|list`: validate or list project-record exceptions
- `rust-qa summary [--check]`: regenerate (or check) the agent summary region
- `rust-qa tools`: the tool catalogue

## Writing rules

Write each rule against the richest layer that can express it.

1. **dylint (primary)**: late lint passes with full rustc type information (`LateContext`,
   `typeck_results`, trait queries). Create a library with `cargo dylint new`, keep its own pinned
   nightly `rust-toolchain.toml` (isolated from the product toolchain), prove it with
   `dylint_testing` UI tests, and list it under `[workspace.metadata.dylint] libraries`. Declare
   lints at `Deny` and choose stable, namespaced names.
2. **ast-grep (supplementary)**: YAML rules for cheap single-file checks where syntax provably
   suffices. Set `severity: error` to block. Record in the rule's doc why syntax suffices. List
   rule directories under `ruleDirs` in `sgconfig.yml` at the workspace root.

`rust-qa` also ships a small ast-grep pack, on by default, that runs beside the project's rules
(a second `ast-grep scan`, so neither set hides the other's findings; it needs no `sgconfig.yml`
and honours `scan_exclude`). Each rule is a cheap single-file check that no Clippy or rustc lint
covers, and its doc says why syntax suffices:

| Rule | Flags |
|---|---|
| `rqaci.ast.shell-c-command` | `Command::new("sh")` (or another shell) given `-c` and a command string |
| `rqaci.ast.hardcoded-temp-path` | string literals under `/tmp` or `/var/tmp` |
| `rqaci.ast.tls-verification-disabled` | `danger_accept_invalid_certs(true)` / `danger_accept_invalid_hostnames(true)` |

Switch the pack off with `[ast_grep] bundled = false`; project rules keep running. Silence one
finding with `// ast-grep-ignore: <id>` plus a project-record exception for `ast-grep::<id>`.
Keep project rule ids out of the `rqaci.` namespace.

Every rule identifier needs a document `<docs_dir>/<id>.md` (`::` becomes `.`) whose sections say what
it flags, why, and how to fix correctly. `rust-qa docs-check` enforces this.

## Project record

`qa/record.toml` holds every exception; nothing else can silence a defence:

```toml
[[exception]]
rule = "clippy::needless_pass_by_value"   # identifier exactly as printed
path = "src/handlers/upload.rs"           # file or glob relative to the root
justification = "The framework requires owned extractors; borrowing does not compile here."
decided_by = "maintainer"                 # optional, never empty
```

Empty, generic ("TODO", "legacy", "needed for now", ...) or very short justifications are rejected.
Exceptions that match nothing are reported as stale.

## Configuration

`qa/rust-qa.toml` (optional; unknown keys are rejected). Defaults:

```toml
record = "qa/record.toml"
docs_dir = "docs/defences"
scan_exclude = []            # globs excluded from the suppression scan and the bundled ast-grep
                             # pack; target/ and .git/ are always skipped

[summary]
# file = "AGENTS.md"         # agent-loaded file that receives the generated region; unset = off

[tools]                      # explicit `-t` requests override these switches
fmt = true
record = true
suppression = true
summary = true
docs = true
ast_grep = true
clippy = true
dylint = true
deny = true
audit = false
machete = true
test = true
coverage = true
mutants = true

[ast_grep]
bundled = true               # run the bundled rqaci.ast.* pack beside the project's own rules

[coverage]
fail_under_lines = 80

[mutants]
mode = "off"                 # off | diff | full
base = "origin/main"         # diff mode compares against the merge base with this ref
```

A tool whose own configuration is absent (no dylint metadata, no `deny.toml`; for ast-grep, no
`sgconfig.yml` and the bundled pack off) reports itself as skipped.

## Installing

Pin an exact tag. Install the binary and the pinned external tools:

```bash
cargo install --locked --git https://github.com/LongTermSupport/rust-qa-ci --tag v0.1.1 rust-qa
git clone --depth 1 --branch v0.1.1 https://github.com/LongTermSupport/rust-qa-ci /tmp/rust-qa-ci
/tmp/rust-qa-ci/scripts/install-tools.sh            # versions from tools.lock
```

Bundled type-aware rules load from the same tag:

```toml
[workspace.metadata.dylint]
libraries = [
    { git = "https://github.com/LongTermSupport/rust-qa-ci", tag = "v0.1.1", pattern = "lints/rqaci_lints" },
]
```

## Public-safety guard

This repository must stay safe to publish. `scripts/public-safety-scan.sh` scans the tree, staged
changes, commit messages, history or arbitrary text against a denylist supplied from outside the
repository (`RQACI_DENYLIST`). Enable the hooks with `git config core.hooksPath .githooks`. The
pre-publication audit is [docs/PUBLICATION-CHECKLIST.md](docs/PUBLICATION-CHECKLIST.md).
