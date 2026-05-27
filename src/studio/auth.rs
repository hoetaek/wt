use anyhow::{Context, Result};
use axum::http::{HeaderMap, header};
use std::sync::atomic::{AtomicBool, Ordering};

pub const COOKIE_NAME: &str = "wt_studio_session";

#[derive(Debug)]
pub struct StudioSession {
    token: String,
    origin: String,
    auth_consumed: AtomicBool,
}

impl StudioSession {
    pub fn new(origin: String, token: String) -> Self {
        Self {
            token,
            origin,
            auth_consumed: AtomicBool::new(false),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn accept_auth_token(&self, token: &str) -> bool {
        token == self.token
            && self
                .auth_consumed
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }

    pub fn validate_api_headers(&self, headers: &HeaderMap) -> bool {
        origin_matches(headers, &self.origin) && session_cookie_matches(headers, &self.token)
    }
}

pub fn mint_session_token() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("failed to mint wt studio session token")?;
    Ok(hex_encode(&bytes))
}

fn origin_matches(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == expected)
}

fn session_cookie_matches(headers: &HeaderMap, expected_token: &str) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|cookie| {
            cookie.split(';').any(|part| {
                let Some((name, value)) = part.trim().split_once('=') else {
                    return false;
                };
                name == COOKIE_NAME && value == expected_token
            })
        })
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_session_token_is_256_bits_as_hex() {
        let token = mint_session_token().unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn auth_token_is_accepted_once() {
        let session = StudioSession::new("http://127.0.0.1:8424".into(), "secret".into());

        assert!(session.accept_auth_token("secret"));
        assert!(!session.accept_auth_token("secret"));
        assert!(!session.accept_auth_token("wrong"));
    }
}
