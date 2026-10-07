# rqaci_lints

Type-aware [Dylint](https://github.com/trailofbits/dylint) lint library bundled with rust-qa-ci.
Every lint runs as a late lint pass, so it sees resolved types (aliases expanded, generic
arguments substituted) rather than source text.

## Lints

| Lint | Level | Doc |
| --- | --- | --- |
| `rqaci_sensitive_debug_derive` | deny | [rqaci_sensitive_debug_derive.md](../../crates/rust-qa/docs/rules/rqaci_sensitive_debug_derive.md) |
| `rqaci_suppression_generated` | deny | [rqaci_suppression_generated.md](../../crates/rust-qa/docs/rules/rqaci_suppression_generated.md) |

## Development

The toolchain is pinned in `rust-toolchain.toml`; `cargo-dylint` and `dylint-link` must be on
`PATH`.

```sh
cargo test                               # UI tests in ui/ and ui_config/
cargo clippy --all-targets -- -D warnings
```

Each lint keeps red-proof UI fixtures: a `.rs` file under `ui/` with the expected diagnostics
in the matching `.stderr` file. `ui_config/` holds fixtures run with a non-default
`dylint.toml`.
