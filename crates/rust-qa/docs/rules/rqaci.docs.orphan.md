# rqaci.docs.orphan

## What it flags

A page in the project's documentation directory (`docs_dir`, `docs/defences` by default) that
documents no identifier `rust-qa rules` lists: its file name, with `.` read as `::` and compared as a
lint name (without case, `-` read as `_`), matches no active defence. `README.md` is left alone.

## Why

A page outlives the rule it described when the rule is removed or renamed. A reader who finds the
page, or an agent that loads it, then follows a defence that no longer runs, and the documentation
directory stops being a true list of what the gate enforces.

## How to fix correctly

Delete the page if its defence is gone. If the defence was renamed, rename the page to match the
identifier it prints. If the defence should still run, restore it in the configuration so that it is
listed again.

## Why syntax suffices

It compares file names in one directory with the identifiers the listing derives from the
configuration; no code is read.

## Suppression

Not suppressible; delete, rename or restore.
