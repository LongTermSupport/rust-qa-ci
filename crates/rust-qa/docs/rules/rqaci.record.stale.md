# rqaci.record.stale

## What it flags

A project-record exception that matches no suppression in the workspace (checked on full,
unscoped runs only).

## Why

A stale exception is a pre-approved hole: the next suppression that happens to match it passes
without anyone deciding it. Exceptions must disappear with the code that needed them.

## How to fix correctly

Delete the exception from the project record. If the suppression moved, update `path` to its new
location and re-read the justification to confirm it still holds.

## Why syntax suffices

It compares the record against the suppressions found by `rqaci.suppression.unrecorded`.

## Suppression

Not suppressible. Remove or correct the record entry.
