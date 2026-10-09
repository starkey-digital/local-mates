use std::time::Duration;

use bytes::Bytes;
use iroh::endpoint::Connection;

pub const ALPN: &[u8] = b"local-mates/0";

/// Unreliable send: games expect UDP-like loss, and TCP inside the tunnel retransmits itself.
pub fn send(conn: &Connection, pkt: Bytes) {
    if let Err(err) = conn.send_datagram(pkt) {
        tracing::debug!("dropped packet to {}: {err}", conn.remote_id().fmt_short());
    }
}

/// Prints whenever a peer's connection switches between direct and relayed.
pub async fn report_path(conn: Connection, who: String) {
    let mut last = None;
    while conn.close_reason().is_none() {
        let paths = conn.paths();
        if let Some(path) = paths.iter().find(|p| p.is_selected()) {
            let kind = if path.is_relay() { "relayed" } else { "direct" };
            if last != Some(kind) {
                println!("{who}: {kind} ({}ms)", path.rtt().as_millis());
                last = Some(kind);
            }
        }
        drop(paths);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
