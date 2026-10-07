# rqaci.suppression.allow

## What it flags

A lint silenced with an `allow` attribute (`#[allow(...)]`, `#![allow(...)]`, or `allow` inside
`cfg_attr`) in Rust source. It is reported in addition to `rqaci.suppression.unrecorded`, so a
recorded `allow` still fails.

## Why

`allow` never expires: when the code changes and the lint stops firing, the attribute stays,
silently pre-approving the next violation at that site. `expect` (stable since Rust 1.81) fails
with `unfulfilled_lint_expectations` as soon as the lint no longer fires, so the exception
removes itself, and its `reason` documents the decision next to the code.

## How to fix correctly

Replace `allow(lint)` with `expect(lint, reason = "...")` and keep the matching project-record
exception. If the lint fires only in some configurations, scope the attribute with `cfg_attr` so
the expectation holds wherever it applies.

## Why syntax suffices

Lint attributes are syntax; no type information changes which attribute was written.

## Suppression

Where `expect` cannot work (for example, code generated into a crate you do not compile with the
same lint set), record an exception for `rqaci.suppression.allow` at that file. It counts against
the record budget like any other.
