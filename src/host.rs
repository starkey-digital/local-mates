use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{Arc, RwLock},
};

use anyhow::Result;
use bytes::Bytes;
use iroh::{Endpoint, endpoint::Connection};
use tokio::task::JoinSet;
use tun_rs::AsyncDevice;

use crate::{
    ipc::{Event, Events},
    link,
    packet::{self, HOST_IP},
    tun,
};

type Peers = Arc<RwLock<HashMap<Ipv4Addr, Connection>>>;

/// The host is the hub: every joiner connects to it and it switches packets between them.
pub async fn run(endpoint: Endpoint, events: Events) -> Result<()> {
    let tun = Arc::new(tun::open(HOST_IP)?);
    let peers = Peers::default();

    let _ = events.send(Event::Hosting {
        code: endpoint.id().to_string(),
        ip: HOST_IP,
    });

    // Owned here so ending the session aborts every task and releases the adapter.
    let mut tasks = JoinSet::new();
    tasks.spawn(tun_to_peers(tun.clone(), peers.clone()));

    while let Some(incoming) = endpoint.accept().await {
        let (tun, peers, events) = (tun.clone(), peers.clone(), events.clone());
        tasks.spawn(async move {
            if let Err(err) = admit(incoming, tun, peers, events).await {
                tracing::warn!("peer failed to join: {err:#}");
            }
        });
        while tasks.try_join_next().is_some() {}
    }
    Ok(())
}

async fn admit(
    incoming: iroh::endpoint::Incoming,
    tun: Arc<AsyncDevice>,
    peers: Peers,
    events: Events,
) -> Result<()> {
    let conn = incoming.accept()?.await?;
    let Some(ip) = claim_ip(&peers, &conn) else {
        conn.close(1u32.into(), b"session full");
        return Ok(());
    };

    let who = format!("{ip} ({})", conn.remote_id().fmt_short());
    let res = serve(&conn, ip, &who, &tun, &peers, &events).await;
    peers.write().unwrap().remove(&ip);
    let _ = events.send(Event::Peer { who, joined: false });
    res
}

async fn serve(
    conn: &Connection,
    ip: Ipv4Addr,
    who: &str,
    tun: &AsyncDevice,
    peers: &Peers,
    events: &Events,
) -> Result<()> {
    let mut assign = conn.open_uni().await?;
    assign.write_all(&ip.octets()).await?;
    assign.finish()?;

    let _ = events.send(Event::Peer {
        who: who.to_owned(),
        joined: true,
    });
    tokio::spawn(link::report_path(
        conn.clone(),
        who.to_owned(),
        events.clone(),
    ));

    while let Ok(pkt) = conn.read_datagram().await {
        // Drop spoofed sources so a peer can't impersonate another.
        if packet::ipv4_src_dst(&pkt).is_some_and(|(src, _)| src == ip) {
            route(pkt, Some(ip), tun, peers).await;
        }
    }
    Ok(())
}

fn claim_ip(peers: &Peers, conn: &Connection) -> Option<Ipv4Addr> {
    let mut peers = peers.write().unwrap();
    let ip = (2..=254)
        .map(packet::peer_ip)
        .find(|ip| !peers.contains_key(ip))?;
    peers.insert(ip, conn.clone());
    Some(ip)
}

async fn tun_to_peers(tun: Arc<AsyncDevice>, peers: Peers) {
    let mut buf = vec![0; tun::MTU as usize];
    while let Ok(n) = tun.recv(&mut buf).await {
        route(Bytes::copy_from_slice(&buf[..n]), None, &tun, &peers).await;
    }
}

/// `from` is the sending peer, or `None` when the packet came from the host's own adapter.
async fn route(pkt: Bytes, from: Option<Ipv4Addr>, tun: &AsyncDevice, peers: &Peers) {
    let Some((_, dst)) = packet::ipv4_src_dst(&pkt) else {
        return;
    };
    let fanout = packet::is_fanout(dst);

    if from.is_some()
        && (fanout || dst == HOST_IP)
        && let Err(err) = tun.send(&pkt).await
    {
        tracing::debug!("adapter write failed: {err}");
    }

    let peers = peers.read().unwrap();
    if fanout {
        for (_, conn) in peers.iter().filter(|(ip, _)| Some(**ip) != from) {
            link::send(conn, pkt.clone());
        }
    } else if let Some(conn) = peers.get(&dst).filter(|_| Some(dst) != from) {
        link::send(conn, pkt);
    }
}
