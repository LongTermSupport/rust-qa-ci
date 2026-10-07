# clippy::pedantic

## What it flags

The Clippy `pedantic` group: stricter style and correctness lints that are off by default. When a
project sets the group to `warn`, `rust-qa` runs Clippy with `-D warnings`, so every member blocks.
Diagnostics name the member lint (for example `clippy::doc_markdown`), noting it is implied by this
group.

## Why

The group catches lossy casts, missing `# Errors` docs, needless pass-by-value and similar issues that
compound in a large codebase.

## How to fix correctly

Resolve the member lint named in the diagnostic with `rust-qa rule-doc clippy::<member>` and follow
its guidance. Lower one member at a site only with `#[expect(clippy::<member>, reason = "...")]` plus
a project-record exception.

## Why syntax suffices

This entry describes a group; each member lint carries its own analysis on rustc's type information.

## Suppression

Do not lower the whole group. Exceptions are per member lint, per site, recorded.
