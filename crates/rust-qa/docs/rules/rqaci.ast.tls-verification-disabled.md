# rqaci.ast.tls-verification-disabled

Level: error. Engine: ast-grep (bundled pack, on by default).

## What it flags

A call to `danger_accept_invalid_certs(true)` or `danger_accept_invalid_hostnames(true)` with the
literal `true`, the switches TLS client builders (such as `reqwest` and `native-tls`) use to turn
off certificate or hostname verification:

```rust
let client = reqwest::Client::builder()
    .danger_accept_invalid_certs(true)
    .build()?;
```

Not flagged: the same calls with `false` or with a non-literal argument (a configuration value);
those need review, not a syntactic verdict.

## Why

Without certificate and hostname verification, TLS still encrypts but no longer authenticates:
anyone on the network path can present their own certificate and read or alter the traffic. The
switch is usually added to get past a self-signed certificate in development and then ships.

## How to fix correctly

Keep verification on and trust the certificate you actually expect:

```rust
let ca = reqwest::Certificate::from_pem(&std::fs::read("certs/internal-ca.pem")?)?;
let client = reqwest::Client::builder()
    .add_root_certificate(ca)
    .build()?;
```

For a hostname mismatch, fix the certificate (or connect by the name it was issued for) rather
than disabling the check. For local development, issue a certificate from a local development CA
and trust that CA in the same way.

## Why syntax suffices

The method names carry the meaning: `danger_accept_invalid_certs` and
`danger_accept_invalid_hostnames` exist only to disable verification, and their argument here is
the literal `true`, so the call disables it whatever the receiver's type. No other information
can make that call safe. Non-literal arguments are not flagged (false negatives, never false
positives).

No Clippy or rustc lint inspects TLS configuration, so this does not duplicate one.

## Suppression

Only with a project-record exception. Put `// ast-grep-ignore: rqaci.ast.tls-verification-disabled`
on the line above the call, and record it:

```toml
[[exception]]
rule = "ast-grep::rqaci.ast.tls-verification-disabled"
path = "tests/support/loopback.rs"
justification = "Test client for the in-process loopback server with a throwaway certificate; never built outside tests."
```
