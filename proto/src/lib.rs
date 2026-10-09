//! Short room codes and the HTTP API that maps them to a host's iroh endpoint id.
//!
//! A room's code is derived from its endpoint id, so it never changes and the server only has to
//! remember which rooms are online right now.

use serde::{Deserialize, Serialize};

/// No 0/O, 1/I/L or U, so codes survive being read out over voice chat.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
const LEN: usize = 6;

/// How long a room stays listed without the host renewing it.
pub const TTL_SECS: u64 = 180;
pub const RENEW_SECS: u64 = 60;

/// A short room code like `K7M-Q2X`, stored without the dash.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Code(String);

impl Code {
    pub fn for_endpoint(endpoint_id: &[u8; 32]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key("local-mates room code v1");
        hasher.update(endpoint_id);
        let mut stream = hasher.finalize_xof();

        // Rejection sampling keeps every character equally likely.
        let limit = (256 / ALPHABET.len() * ALPHABET.len()) as u8;
        let mut code = String::with_capacity(LEN);
        while code.len() < LEN {
            let mut b = [0];
            stream.fill(&mut b);
            if b[0] < limit {
                code.push(ALPHABET[b[0] as usize % ALPHABET.len()] as char);
            }
        }
        Self(code)
    }

    /// Accepts what people type: any case, with or without the dash or spaces.
    pub fn parse(input: &str) -> Option<Self> {
        let code: String = input
            .chars()
            .filter(|c| !matches!(c, '-' | ' '))
            .map(|c| c.to_ascii_uppercase())
            .collect();
        (code.len() == LEN && code.bytes().all(|b| ALPHABET.contains(&b))).then_some(Self(code))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (a, b) = self.0.split_at(LEN / 2);
        write!(f, "{a}-{b}")
    }
}

/// Decodes the 64-char hex form iroh displays endpoint ids in.
pub fn parse_endpoint_id(hex: &str) -> Option<[u8; 32]> {
    let mut out = [0; 32];
    if hex.len() != 64 {
        return None;
    }
    for (byte, pair) in out.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// Body of `PUT /v1/rooms/{code}` (announce/renew) and reply to `GET /v1/rooms/{code}`.
#[derive(Serialize, Deserialize)]
pub struct Room {
    pub endpoint_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_stable_per_endpoint() {
        let a = Code::for_endpoint(&[1; 32]);
        assert_eq!(a, Code::for_endpoint(&[1; 32]));
        assert_ne!(a, Code::for_endpoint(&[2; 32]));
        assert_eq!(Code::parse(&a.to_string()), Some(a));
    }

    /// Room codes are persistent: changing the derivation would change everyone's code.
    #[test]
    fn code_derivation_is_frozen() {
        assert_eq!(Code::for_endpoint(&[0x0f; 32]).to_string(), "7Q9-HT8");
    }

    #[test]
    fn parse_is_forgiving() {
        let code = Code::parse("k7m-q2x").unwrap();
        assert_eq!(code.to_string(), "K7M-Q2X");
        assert_eq!(Code::parse(" K7M Q2X "), Some(code));
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert_eq!(Code::parse("K7M-Q2"), None);
        assert_eq!(Code::parse("K7M-Q2O"), None); // O isn't in the alphabet
        assert_eq!(Code::parse("abc123def456"), None);
    }

    #[test]
    fn parses_endpoint_hex() {
        let hex = "0f".repeat(32);
        assert_eq!(parse_endpoint_id(&hex), Some([0x0f; 32]));
        assert_eq!(parse_endpoint_id("0f"), None);
        assert_eq!(parse_endpoint_id(&"zz".repeat(32)), None);
    }
}
