# rqaci_sensitive_debug_derive

Level: deny. Library: `rqaci_lints` (Dylint, late lint pass).

## What it flags

A `#[derive(Debug)]` on a struct or enum in the crate being checked when either:

- the type itself is sensitive (its simple name matches a sensitive pattern) and it holds at
  least one field, for example `#[derive(Debug)] struct ApiToken(String);`; or
- any field's resolved type contains a sensitive type whose own `Debug` is derived. The search
  follows type aliases, references, tuples, arrays and generic arguments, so `token: ApiToken`,
  `key: K` with `type K = ApiToken;`, `tokens: Option<Vec<ApiToken>>`, `pair: (u8, &ApiToken)`
  and `batch: [Box<ApiToken>; 2]` all fire. Enum variant fields are reported as `Variant.field`.

Default patterns (case-insensitive substring of the type's simple name): `Secret`, `Password`,
`Passwd`, `Token`, `ApiKey`, `Credential`, `PrivateKey`.

Not flagged:

- a sensitive type with a hand-written `Debug`, and any type that embeds it: the hand-written
  impl is trusted to redact, which is the fix this rule asks for;
- a sensitive-named type with no fields (for example a fieldless `enum TokenKind`), which has
  nothing to leak;
- fields typed by a generic parameter (`struct Holder<T> { inner: T }`): the concrete type is
  unknown at the definition, and the sensitive type's own derive is reported where it is defined;
- containers with a hand-written `Debug`.

When a sensitive type derives `Debug`, both that type and each type embedding it are reported.
Fixing the sensitive type (hand-written redacting `Debug`) clears all of them.

## Why

Derived `Debug` prints every field verbatim. `{:?}` reaches logs, panic messages, error reports
and test output, so a derived `Debug` on anything holding a credential is a leak waiting for its
first `tracing::debug!(?value)`.

## How to fix correctly

Write `Debug` by hand on the sensitive type and redact its contents. Types that embed it can then
keep `#[derive(Debug)]`.

```rust
use std::fmt;

pub struct ApiToken(String);

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiToken(<redacted>)")
    }
}

#[derive(Debug)] // fine: `ApiToken`'s own `Debug` redacts
pub struct Client {
    pub endpoint: String,
    pub token: ApiToken,
}
```

When the sensitive type comes from another crate and derives `Debug` there, redact in the
container instead:

```rust
use std::fmt;

pub struct Client {
    pub endpoint: String,
    pub token: other_crate::ApiToken,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}
```

Do not rename the type to dodge the pattern; the name is what tells readers it is sensitive.

## Why type-aware

A text or syntax rule sees `key: K` and cannot know that `K` is an alias for `ApiToken`. It also
cannot tell whether `ApiToken`'s `Debug` is derived or hand-written, because that is decided by
an impl that may live in another module or crate. This rule runs after type checking: it reads
each field's resolved type from the compiler, walks its generic arguments, and asks the trait
system which `Debug` impl applies and whether that impl is `#[automatically_derived]`.

## Configuration

In `dylint.toml` at the workspace root:

```toml
[rqaci_sensitive_debug_derive]
sensitive_type_patterns = ["Secret", "Password", "Token", "Nonce"]
```

`sensitive_type_patterns` replaces the defaults; it does not extend them. Copy the defaults
into the list to keep them. Matching is a case-insensitive substring test on the type's simple
name, so `Token` also matches `Tokenizer`; such types are false positives handled by narrower
patterns or a recorded suppression.

## Suppression

Only with `#[expect]` plus a matching exception in the project record. A bare `#[allow]` is
forbidden and is itself reported by the suppression guard.

Because the lint only exists while Dylint runs, gate the attribute on the library so plain
`rustc` does not report an unknown lint:

```rust
#[cfg_attr(
    dylint_lib = "rqaci_lints",
    expect(rqaci_sensitive_debug_derive, reason = "fixed-format test token, never a real secret")
)]
#[derive(Debug)]
pub struct FixtureToken(String);
```

The attribute can sit on the type or on the reported field. `expect` fails the build once the
finding goes away, so stale suppressions do not accumulate.
