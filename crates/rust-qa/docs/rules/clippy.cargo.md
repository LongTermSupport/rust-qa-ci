# clippy::cargo

## What it flags

The Clippy `cargo` group: manifest metadata and dependency hygiene (missing package metadata,
multiple versions of one crate, wildcard dependencies). Diagnostics name the member lint.

## Why

Clean manifests make builds reproducible and dependency trees auditable.

## How to fix correctly

Resolve the member lint with `rust-qa rule-doc clippy::<member>` and correct the manifest it names.

## Why syntax suffices

This entry describes a group; members read Cargo metadata.

## Suppression

Exceptions are per member lint and recorded.
