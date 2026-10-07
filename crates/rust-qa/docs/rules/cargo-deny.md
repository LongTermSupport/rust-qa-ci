# cargo-deny

## What it flags

`cargo deny check` failures per `deny.toml`: security advisories, disallowed licences, banned or
duplicate crates, and dependencies from unapproved sources. cargo-deny prints its own codes (for
example `vulnerability`, `rejected`, `banned`).

## Why

Dependencies are code you ship. A gate on advisories, licences and sources stops known-bad or
non-compliant crates from entering the build.

## How to fix correctly

For an advisory, upgrade to a patched version (`cargo update -p <crate>`) or replace the crate. For a
licence rejection, replace the crate or, if the licence is acceptable, add it to the allow list in
`deny.toml` with the decision recorded. For a ban or duplicate, align versions or replace the crate.

## Why syntax suffices

Not applicable: cargo-deny is a fixed auditor run as a runner gate, not a project-authored detector.

## Suppression

Every `[advisories] ignore` entry needs a project-record exception (`cargo-deny::<advisory id>`).
