# tests

## What it flags

A failing test: `cargo nextest run` across the selected packages, then doctests (`cargo test --doc`)
when a library target exists. Runs only after every detector has passed.

## Why

Tests are runners, not detectors: they prove behaviour of the code paths they exercise. They come
after the static defences so a run never spends time executing code that a detector already rejects.

## How to fix correctly

Read the failing test's output, reproduce it with `cargo nextest run <test name>`, and fix the code
(or the test, when the test encodes the wrong behaviour). Never retry a flaky test into passing; make
it deterministic or remove it.

## Why syntax suffices

Not applicable: this is a runner.

## Suppression

Do not mark tests ignored to pass the gate; an ignored test needs a project-record exception.
