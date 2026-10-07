# rqaci.record.budget

## What it flags

A project record over one of the budgets set in the `[record]` table of `qa/rust-qa.toml`:

- `max_total`: exceptions in the whole record (also caps `[ratchet] high_water`);
- `max_per_rule`: exceptions for one rule id (hyphenated and underscored lint names count as one);
- `max_policy`: broad, glob-path exceptions marked `policy = true`;
- `[record.trust.<tier>] max_count`: exceptions written by models of one trust tier.

## Why

Exception records only ever grow unless something stops them. Each entry is cheap to add, nobody
owns its removal, and a long list stops being read. A hard cap turns "add one more" into "fix one
first". A per-rule cap catches the pattern where a rule is excepted site by site instead of fixed
at the source. Broad exceptions silence a rule for many files at once and get the smallest cap.

## How to fix correctly

Shrink the record: fix the code behind an exception and delete the entry, starting with the rule
that has the most entries. If one rule needs many exceptions, the rule or the code pattern is
wrong: change the pattern (a helper, a type) or, as a deliberate decision, the lint level in the
lint table, not the record. Raising a budget is a policy change for a human to make in
`qa/rust-qa.toml` with its rationale; it is never the fix for a failing gate.

## Why syntax suffices

The record is a TOML file; the check counts its entries.

## Suppression

Not suppressible.
