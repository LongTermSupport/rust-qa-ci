# clippy::all

## What it flags

The default Clippy groups together (`correctness`, `suspicious`, `style`, `complexity`, `perf`).
Diagnostics name the member lint.

## Why

These are Clippy's on-by-default lints: likely bugs, suspicious code, and needless complexity or cost.

## How to fix correctly

Resolve the member lint named in the diagnostic with `rust-qa rule-doc clippy::<member>` and follow
its guidance.

## Why syntax suffices

This entry describes a group; each member lint carries its own analysis on rustc's type information.

## Suppression

Exceptions are per member lint, per site, recorded in the project record.
