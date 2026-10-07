# warnings

## What it flags

Every rustc and Clippy lint at warn level. `rust-qa` runs Clippy with `-D warnings`, so any warning,
including every member of a warn-level group such as `clippy::pedantic`, fails the gate. The
diagnostic names the specific lint; resolve that identifier with `rust-qa rule-doc <id>`.

## Why

A warning that does not block is ignored. Making warnings blocking turns the configured lint set into
a defence rather than advice.

## How to fix correctly

Read the specific lint named in the diagnostic (`rust-qa rule-doc <lint>`) and change the code as its
documentation describes. If the lint is wrong at this site, use a narrow
`#[expect(<lint>, reason = "...")]` and add a project-record exception for that lint id.

## Why syntax suffices

This is the rustc lint group itself; the member lints carry their own type-aware analysis.

## Suppression

Never lower `warnings` globally. Exceptions are per lint, per site, recorded.
