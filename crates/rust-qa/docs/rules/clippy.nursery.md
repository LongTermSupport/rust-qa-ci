# clippy::nursery

## What it flags

The Clippy `nursery` group: lints still under development. Diagnostics name the member lint.

## Why

Projects opt in to catch issues early, accepting a higher false-positive rate.

## How to fix correctly

Resolve the member lint with `rust-qa rule-doc clippy::<member>`. If a member is unreliable for the
project, set that one member to `allow` in the Cargo `[lints]` table and record the exception with
the false-positive evidence.

## Why syntax suffices

This entry describes a group; each member lint carries its own analysis.

## Suppression

Exceptions are per member lint and recorded.
