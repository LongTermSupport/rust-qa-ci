# rqaci.record.invalid

## What it flags

A project-record exception that is malformed: no rule, no path, an invalid path glob, an empty
`decided_by`, or a justification that is empty, generic ("TODO", "legacy", "needed for now",
"temporary", "false positive" and similar), contains a placeholder (TODO, TBD, FIXME, XXX), or is too
short to explain anything.

## Why

An exception without a real reason is indistinguishable from a silenced defect. The record is only
useful if every entry says why the rule is wrong at that site and why the exception is safe.

## How to fix correctly

Rewrite the justification to answer two questions in a full sentence: why does the rule not apply
here, and what makes the exception safe? Name the constraint (an external API, a measured cost, a
proof) rather than the feeling ("needed", "legacy"). Keep `path` as narrow as possible, ideally one
file. If you cannot write such a justification, remove the exception and fix the code instead.

## Why syntax suffices

The record is a TOML file; validation is about its content, not program types.

## Suppression

Not suppressible. Fix the record entry.
