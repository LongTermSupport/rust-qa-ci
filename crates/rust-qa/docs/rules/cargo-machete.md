# cargo-machete

## What it flags

Dependencies declared in a `Cargo.toml` that no source file uses, reported by `cargo machete`.

## Why

Unused dependencies cost build time and widen the supply-chain surface for no benefit.

## How to fix correctly

Remove the dependency from the manifest. If it is used in a way the tool cannot see (for example
only through a macro or a build script), keep it and record the exception.

## Why syntax suffices

cargo-machete searches source text for crate names; it is a runner gate, not a project-authored
detector.

## Suppression

`[package.metadata.cargo-machete] ignored` entries need a project-record exception.
