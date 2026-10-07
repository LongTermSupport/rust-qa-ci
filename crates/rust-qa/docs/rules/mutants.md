# mutants

## What it flags

A missed mutant: cargo-mutants changed the code (for example replaced a function body or flipped a
comparison) and every test still passed. In `diff` mode only code changed relative to the configured
base is mutated; in `full` mode the whole workspace.

## Why

A surviving mutant shows code that is executed but not asserted on. It is the gap coverage cannot see.

## How to fix correctly

Read the mutant in `mutants.out/missed.txt`, then add or strengthen a test that fails for that
mutation. Assert on outcomes, not on calls. If the mutant is semantically equivalent to the original,
record it as an exception rather than adding a meaningless assertion.

## Why syntax suffices

Not applicable: this is a runner.

## Suppression

`exclude_re` or `#[mutants::skip]` entries require a project-record exception.
