# unused-suppression

## What it flags

ast-grep's own warning: an `ast-grep-ignore` comment that suppresses no finding, because the rule it
names no longer matches there, or no rule ever did.

## Why

A suppression that silences nothing is noise at best. At worst it waits for the next real finding at
that site and hides it, with a project-record exception that was justified for code that has since
changed.

## How to fix correctly

Delete the comment, and delete the project-record exception for `ast-grep::<rule id>` at that path
that licensed it (`rust-qa record prune` lists and removes exceptions that cover nothing).

## Why syntax suffices

ast-grep reports it from the comments and the matches it already has.

## Suppression

Not applicable: remove the comment.
