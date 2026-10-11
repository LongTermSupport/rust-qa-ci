# rqaci.dylint.nameless

## What it flags

A finding at a source location, printed while a dylint library checks the code, that carries no lint
name: the library reported it directly (for example with `span_warn` on the compiler's diagnostic
context) rather than through a lint it declares.

## Why

A finding with no lint name cannot be listed, documented, forced to warn, levelled or covered by a
project-record exception, and a lane that keeps only named lints would drop it, so the rule it comes
from would pass the gate silently.

## How to fix correctly

Report the finding through a lint the library declares (`declare_lint!` and `span_lint` from
`clippy_utils` or `LateContext::emit_span_lint`), and give that lint its page.

## Why syntax suffices

It reads the compiler's own diagnostic stream.

## Suppression

Not suppressible; report through a declared lint.
