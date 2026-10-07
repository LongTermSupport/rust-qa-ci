# rqaci.suppression.unrecorded

## What it flags

A route that silences a defence without a matching exception in the project record:

- `#[allow(...)]` or `#[expect(...)]` lint attributes, including inside `#[cfg_attr(...)]` and inside
  macro bodies;
- `// ast-grep-ignore` comments (bare or with rule ids);
- `allow` levels in Cargo `[lints]` or `[workspace.lints]` tables;
- `-A`, `--allow` or `--cap-lints` in `.cargo/config.toml` rustflags;
- `allow-*` switches set to `true` in `clippy.toml`;
- `[advisories] ignore` entries in `deny.toml`;
- a Rust file that cannot be parsed, because its suppressions cannot be verified.

## Why

A suppression is an exception to a defence. Exceptions are decisions, and a decision needs a written
reason in one discoverable place, otherwise defences erode silently one line at a time.

## How to fix correctly

1. Prefer fixing the code so the defence passes; that removes the need for any exception.
2. If the rule is genuinely wrong at this site, keep the narrowest suppression (one item, one lint),
   written as `#[expect(lint, reason = "...")]` so it fails once it stops being needed.
3. Add an exception to the project record (`qa/record.toml` by default):

   ```toml
   [[exception]]
   rule = "clippy::needless_pass_by_value"
   path = "src/handlers/upload.rs"
   justification = "The framework requires owned extractors; borrowing does not compile here."
   decided_by = "maintainer"
   ```

   `rule` is the identifier exactly as the defence prints it; `path` is a file or glob relative to the
   workspace root. The record validator rejects empty or generic justifications.

## Why syntax suffices

Suppressions are syntax and configuration by definition: types cannot add or remove one. The guard
parses every Rust file fully (not a text search), so string literals and doc text are not mistaken
for suppressions, and walks macro token trees so attributes inside macro bodies are found.
Attributes that a macro assembles from fragments exist only after expansion; the bundled dylint
lint `rqaci_suppression_generated` covers those.

## Suppression

This rule cannot itself be suppressed; disable it only through the tool switch in
`qa/rust-qa.toml`, which is visible in `rust-qa rules`.
