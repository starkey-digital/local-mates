use std::time::Duration;

use anyhow::anyhow;
use bytes::Bytes;
use iroh::endpoint::{ApplicationClose, Connection, ConnectionError};

use crate::ipc::{Event, Events};

pub const ALPN: &[u8] = b"local-mates/0";

/// Close codes the host uses to tell a joiner why it was turned away.
pub const CLOSE_FULL: u32 = 1;
pub const CLOSE_DENIED: u32 = 2;

/// Longest device or room name sent over the wire, in bytes.
pub const MAX_NAME: usize = 64;

pub fn device_name() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// Truncated on a character boundary to fit the peer's read limit.
pub fn encode_name(name: &str) -> &[u8] {
    let mut end = name.len().min(MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name.as_bytes()[..end]
}

/// Names come from the other side, so strip anything that could mess with a terminal or UI.
pub fn decode_name(bytes: &[u8], fallback: &str) -> String {
    let name: String = String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    match name.trim() {
        "" => fallback.into(),
        name => name.into(),
    }
}

pub fn explain_close(err: ConnectionError) -> anyhow::Error {
    match &err {
        ConnectionError::ApplicationClosed(ApplicationClose { error_code, .. }) => {
            match u64::from(*error_code) {
                code if code == CLOSE_DENIED as u64 => anyhow!("the host didn't let you in"),
                code if code == CLOSE_FULL as u64 => anyhow!("the room is full"),
                _ => anyhow!("the host closed the connection"),
            }
        }
        _ => anyhow::Error::new(err).context("lost connection to the host"),
    }
}

/// Unreliable send: games expect UDP-like loss, and TCP inside the tunnel retransmits itself.
pub fn send(conn: &Connection, pkt: Bytes) {
    if let Err(err) = conn.send_datagram(pkt) {
        tracing::debug!("dropped packet to {}: {err}", conn.remote_id().fmt_short());
    }
}

/// Reports whenever a peer's connection switches between direct and relayed.
pub async fn report_path(conn: Connection, who: String, events: Events) {
    let mut last = None;
    while conn.close_reason().is_none() {
        let paths = conn.paths();
        if let Some(path) = paths.iter().find(|p| p.is_selected()) {
            let relayed = path.is_relay();
            if last != Some(relayed) {
                let _ = events.send(Event::Path {
                    who: who.clone(),
                    relayed,
                    rtt_ms: path.rtt().as_millis() as u64,
                });
                last = Some(relayed);
            }
        }
        drop(paths);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_truncates_on_char_boundary() {
        let name = "é".repeat(40); // 80 bytes
        let encoded = encode_name(&name);
        assert!(encoded.len() <= MAX_NAME);
        assert!(std::str::from_utf8(encoded).is_ok());
    }

    #[test]
    fn decode_strips_control_chars() {
        assert_eq!(decode_name(b"Bob\x1b[2J\n", "x"), "Bob[2J");
        assert_eq!(decode_name(b"  \n ", "fallback"), "fallback");
    }
}
