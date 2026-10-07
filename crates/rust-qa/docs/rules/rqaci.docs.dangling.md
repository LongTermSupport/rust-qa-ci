# rqaci.docs.dangling

## What it flags

An identifier listed by `rust-qa rules` that resolves to no documentation: no project document
(`<docs_dir>/<id>.md`, with `::` written as `.`), no bundled document, and nothing from the installed
Clippy or rustc.

## Why

A failing defence must tell the reader what is wrong and how to fix it. An identifier without a
document is a dead end: the reader guesses, and a guessed fix is often a suppression.

## How to fix correctly

Write `<docs_dir>/<id>.md` with sections: what it flags, why, how to fix correctly, why the rule is
type-aware or why syntax suffices, and how (if ever) it may be suppressed. Keep the rule and its
document in the same commit.

## Why syntax suffices

It checks documentation files against the active defence list.

## Suppression

Not suppressible; write the document.
