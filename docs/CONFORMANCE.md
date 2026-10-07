# Defence Before Fix conformance

Status: **partial**. This toolchain does not claim conformance. The machine-readable declaration is
`[workspace.metadata.defence-before-fix]` in the root `Cargo.toml`; this page is the evidence record.

Specs: method 1.1.0, detector 1.0.0, toolchain 0.2.0 (https://defence-before-fix.github.io/).

## Toolchain MUSTs

| Clause | Requirement | Status | Evidence |
|---|---|---|---|
| 4.1 | Every detector routed through conforms | Gap | dylint (bespoke, type-aware) and ast-grep (bespoke, syntactic) are the authoring engines; Clippy runs off-the-shelf lints through the `rust-qa` wrapper, which adds file subsetting and offline docs |
| 4.2 | Every printable id resolves offline, docs state the correct construction | Partial | `rust-qa rule-doc`, `rust-qa docs-check`; bundled docs carry a "How to fix correctly" section (test-enforced); Clippy via `cargo clippy --explain`; rustc lints via the `rust-docs` component or `rustc -W help` |
| 4.3 | A defence forbids unrecorded suppressions | Met | `rqaci.suppression.unrecorded` covers lint attributes (including `cfg_attr` and macro bodies), ast-grep ignores, Cargo `[lints]` allow levels, rustflags `-A`/`--cap-lints`, `clippy.toml` allow switches, cargo-deny advisory ignores |
| 4.4 | Local, subset-capable (one file), ids printed unaltered | Met with note | `rust-qa run -p <file>`; per-crate engines analyse the owning crate and print only diagnostics in the requested paths |
| 4.5 | Detectors before runners; stop on detector failure | Met | `pipeline` module and its tests; end-to-end test in `tests/main_test.rs` |
| 5.1-5.3 | List active defences without running them, derived from configuration, project defences included | Met | `rust-qa rules` reads Cargo `[lints]`, dylint metadata, `sgconfig.yml` rule dirs and `qa/rust-qa.toml` (which also switches the bundled `rqaci.ast.*` ast-grep pack) |
| 6.1-6.3 | Project record read by the toolchain, justification required, generic text rejected, record enumerable | Met | `qa/record.toml`, `rqaci.record.invalid`, `rqaci.record.stale`, `rust-qa record list`, `rust-qa rules` |
| 8.1 | Release fails if a bundled defence lacks docs | Met as test | `tests/builtin_test.rs`, `tests/docs_test.rs`, `tests/ast_grep_test.rs` (bundled ast-grep rules: docs, plus fixtures proving each fires and stays quiet) |
| 8.2 | Toolchain runs its own defences on its own source | Met | `qa/rust-qa.toml`; `rust-qa run` in this repository loads the bundled dylint library and runs the bundled ast-grep pack |

## SHOULDs

| Clause | Status |
|---|---|
| 6.4 State own defaults | Met: README "Configuration" |
| 7.1-7.2 Agent summary in a delimited generated region | Met: `rust-qa summary` |
