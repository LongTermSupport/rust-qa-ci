# clippy::restriction

## What it flags

The Clippy `restriction` group. Its members contradict each other by design, so enable individual
members (for example `clippy::unwrap_used`) rather than the group. Diagnostics name the member lint.

## Why

Restriction lints forbid otherwise valid constructs that a project has decided against.

## How to fix correctly

Resolve the member lint with `rust-qa rule-doc clippy::<member>` and follow its guidance. Configure
members individually in the Cargo `[lints]` table.

## Why syntax suffices

This entry describes a group; each member lint carries its own analysis.

## Suppression

Exceptions are per member lint and recorded.
