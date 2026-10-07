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
