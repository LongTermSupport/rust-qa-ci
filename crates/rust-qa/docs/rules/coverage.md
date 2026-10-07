# coverage

## What it flags

Line coverage below the threshold in `[coverage] fail_under_lines` (`qa/rust-qa.toml`), measured by
`cargo llvm-cov nextest`.

## Why

Coverage is a floor, not a goal: it catches code that no test executes at all. Mutation testing
(`mutants`) is the check on whether executed code is actually asserted on.

## How to fix correctly

Run `cargo llvm-cov nextest --html` and open `target/llvm-cov/html/index.html` to see unexecuted
lines. Add tests that assert behaviour of those lines, or delete code that is genuinely unreachable.
Do not lower the threshold to pass; changing it is a project decision recorded with its rationale.

## Why syntax suffices

Not applicable: this is a runner.

## Suppression

Excluding code from coverage requires a project-record exception.
