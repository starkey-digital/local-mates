use std::time::Duration;

use bytes::Bytes;
use iroh::endpoint::Connection;

use crate::ipc::{Event, Events};

pub const ALPN: &[u8] = b"local-mates/0";

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
