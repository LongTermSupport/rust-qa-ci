# rqaci.record.ratchet

## What it flags

Problems with the `[ratchet]` table of the project record, the signed ceiling on its size:

- a non-empty record with no `[ratchet]` table;
- more exceptions than `high_water` (the record grew);
- `decided_by` not one of the configured `deciders`, a generic or missing `reason`, or a
  `decided_on` in the future;
- `high_water` higher than in the committed record (`HEAD`, and the merge base with
  `[record] history_base`) without a fresh signature: `decided_on` newer than the previous one and
  within the last 14 days;
- `high_water` above the current count in CI or with `--no-fix` (local runs lower it themselves).

## Why

A cap alone drifts upward: each time it blocks, someone raises it. The ratchet makes shrinkage
automatic and growth a visible, attributed decision. When an entry is removed, the ceiling follows
it down; adding one needs a configured person to raise the ceiling, with a reason, in the same
change.

## How to fix correctly

Prefer not to grow: fix the code instead of adding an exception. If the exception is right, ask a
configured decider to raise `high_water` by one, set `decided_by` to their identifier, `decided_on`
to today and `reason` to why the record must grow. An agent must not sign on a person's behalf.
After removing entries, run `rust-qa run -t record` locally (or `rust-qa record prune`) and commit
the lowered `high_water`.

## Why syntax suffices

It compares the record with configuration and with the committed copy of the same file.

## Suppression

Not suppressible.
