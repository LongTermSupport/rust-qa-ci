#![crate_type = "lib"]

use std::fmt;

// Sensitive by name, derives `Debug` and holds data: fires on the type itself.
#[derive(Debug)]
pub struct ApiToken(pub String);

// Sensitive by name with a hand-written redacting `Debug`: silent, and safe to embed.
pub struct SessionSecret(pub String);

impl fmt::Debug for SessionSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionSecret(<redacted>)")
    }
}

// Sensitive by name but holds no data, so there is nothing to leak: silent.
#[derive(Debug)]
pub enum TokenKind {
    Bearer,
    Basic,
}

pub type K = ApiToken;

#[derive(Debug)]
pub struct DirectField {
    pub label: String,
    pub token: ApiToken,
}

#[derive(Debug)]
pub struct AliasField {
    pub key: K,
}

#[derive(Debug)]
pub struct NestedField {
    pub tokens: Option<Vec<ApiToken>>,
}

#[derive(Debug)]
pub struct TupleArrayRef<'a> {
    pub pair: (u8, &'a ApiToken),
    pub batch: [Box<ApiToken>; 2],
}

#[derive(Debug)]
pub enum Auth {
    Anonymous,
    Bearer(ApiToken),
    Basic { user: String, token: K },
}

// Negatives.

#[derive(Debug)]
pub struct WrapsRedacted {
    pub secret: SessionSecret,
    pub maybe: Option<SessionSecret>,
}

#[derive(Debug)]
pub struct Plain {
    pub name: String,
    pub kind: TokenKind,
}

#[derive(Debug)]
pub struct Holder<T> {
    pub inner: T,
    pub items: Vec<T>,
}

pub struct Client {
    pub endpoint: String,
    pub token: ApiToken,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

// Suppression through `expect` reaches the field-level diagnostic.
#[expect(rqaci_sensitive_debug_derive, reason = "fixture: proves expect is honoured")]
#[derive(Debug)]
pub struct Expected {
    pub token: ApiToken,
}

// Redacting wrappers. UI fixtures cannot have dependencies, so this module stands in for the
// `secrecy` crate; `redacting_types` entries match a suffix of the def path on `::` boundaries.
pub mod secrecy {
    use std::fmt;

    pub struct SecretBox<T: ?Sized>(Box<T>);

    impl<T: ?Sized> fmt::Debug for SecretBox<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("SecretBox(<redacted>)")
        }
    }

    pub type SecretString = SecretBox<str>;
}

// Sensitive name and derived `Debug`, but every field is a redacting type: silent.
#[derive(Debug)]
pub struct ConsentToken(pub secrecy::SecretString);

#[derive(Debug)]
pub struct RefreshToken {
    pub value: Option<Box<secrecy::SecretString>>,
    pub previous: Vec<secrecy::SecretBox<[u8]>>,
}

// Embeds sensitive types whose derived `Debug` only prints redacting fields: silent.
#[derive(Debug)]
pub struct Grant {
    pub consent: ConsentToken,
    pub refresh: Option<RefreshToken>,
}

// One redacting field is not enough when another field prints in clear: fires twice.
#[derive(Debug)]
pub struct MixedToken {
    pub value: secrecy::SecretString,
    pub label: String,
}

#[derive(Debug)]
pub struct HoldsMixed {
    pub token: MixedToken,
}
