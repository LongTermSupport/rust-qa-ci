# cargo-audit

## What it flags

Dependencies in `Cargo.lock` with a RustSec advisory, reported by `cargo audit`.

## Why

A second, independent advisory check alongside cargo-deny; useful on a schedule to surface new
advisories against an unchanged lockfile.

## How to fix correctly

Upgrade the affected crate to a patched version (`cargo update -p <crate>`), or replace it. If no fix
exists, assess exposure and record the decision.

## Why syntax suffices

Not applicable: this is a runner gate.

## Suppression

Ignored advisories need a project-record exception.
