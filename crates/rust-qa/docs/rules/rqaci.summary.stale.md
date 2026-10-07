# rqaci.summary.stale

## What it flags

The generated agent summary region (between the `BEGIN rust-qa-ci generated defences` and
`END rust-qa-ci generated defences` markers in the file named by `[summary] file`) differs from what
the active configuration produces. Reported in CI or `--no-fix` runs; local runs rewrite the region.

## Why

Agents read the summary to learn which defences exist and how to resolve their identifiers. A stale
summary teaches them rules that are gone or hides rules that are active.

## How to fix correctly

Run `rust-qa summary` (or a local `rust-qa run`) and commit the regenerated region. Never edit inside
the markers by hand; change the configuration that the summary is derived from instead.

## Why syntax suffices

The check compares generated text.

## Suppression

Not suppressible; switch the summary off in `qa/rust-qa.toml` if the project has no agent-loaded file.
