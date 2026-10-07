#![crate_type = "lib"]

// The configured list replaces the defaults, so `Token` is not a pattern here.
#[derive(Debug)]
pub struct ApiToken(pub String);

#[derive(Debug)]
pub struct Nonce(pub [u8; 12]);

#[derive(Debug)]
pub struct Request {
    pub nonce: Nonce,
    pub token: ApiToken,
}

pub mod vault {
    use std::fmt;

    pub struct Sealed<T>(T);

    impl<T> fmt::Debug for Sealed<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Sealed(<redacted>)")
        }
    }
}

// `vault::Sealed` is a configured redacting type, so this sensitive newtype is silent.
#[derive(Debug)]
pub struct SealedNonce(pub vault::Sealed<[u8; 12]>);
