use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::Result;
use bytes::Bytes;
use iroh::{Endpoint, EndpointId, endpoint::Connection};
use tokio::{sync::oneshot, task::JoinSet};
use tun_rs::AsyncDevice;

use crate::{
    ipc::{Event, Events},
    link,
    packet::{self, HOST_IP},
    rooms_api,
    session::Shared,
    store::Device,
    tun,
};

type Peers = Arc<RwLock<HashMap<Ipv4Addr, Connection>>>;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// The host is the hub: every joiner connects to it and it switches packets between them.
pub async fn run(endpoint: Endpoint, shared: &Shared) -> Result<()> {
    let tun = Arc::new(tun::open(HOST_IP)?);
    let peers = Peers::default();
    let events = shared.events.clone();
    let room: Arc<str> = format!("{}'s room", link::device_name()).into();

    let _ = events.send(Event::Hosting {
        code: rooms_api::code_for(endpoint.id()).to_string(),
        long_code: endpoint.id().to_string(),
        ip: HOST_IP,
    });

    // Owned here so ending the session aborts every task and releases the adapter.
    let mut tasks = JoinSet::new();
    tasks.spawn(tun_to_peers(tun.clone(), peers.clone()));
    tasks.spawn(rooms_api::announce(shared.http.clone(), endpoint.id()));

    while let Some(incoming) = endpoint.accept().await {
        let (tun, peers, shared, room) = (tun.clone(), peers.clone(), shared.clone(), room.clone());
        tasks.spawn(async move {
            if let Err(err) = admit(incoming, &room, tun, peers, &shared).await {
                tracing::warn!("peer failed to join: {err:#}");
            }
        });
        while tasks.try_join_next().is_some() {}
    }
    Ok(())
}

async fn admit(
    incoming: iroh::endpoint::Incoming,
    room: &str,
    tun: Arc<AsyncDevice>,
    peers: Peers,
    shared: &Shared,
) -> Result<()> {
    let conn = incoming.accept()?.await?;
    let hello = tokio::time::timeout(HELLO_TIMEOUT, async {
        anyhow::Ok(conn.accept_uni().await?.read_to_end(link::MAX_NAME).await?)
    })
    .await??;
    let name = link::decode_name(&hello, "Unknown device");

    if !approve(shared, conn.remote_id(), &name).await? {
        conn.close(link::CLOSE_DENIED.into(), b"not allowed");
        return Ok(());
    }
    let Some(ip) = claim_ip(&peers, &conn) else {
        conn.close(link::CLOSE_FULL.into(), b"room full");
        return Ok(());
    };

    let who = format!("{name} ({ip})");
    let res = serve(&conn, ip, room, &who, &tun, &peers, &shared.events).await;
    peers.write().unwrap().remove(&ip);
    let _ = shared.events.send(Event::Peer { who, joined: false });
    res
}

/// Friends go straight in; anyone else waits for someone at the host to say yes.
async fn approve(shared: &Shared, id: EndpointId, name: &str) -> Result<bool> {
    if shared.store.lock().unwrap().is_friend(&id.to_string()) {
        return Ok(true);
    }

    let (answer, answered) = oneshot::channel();
    shared.approvals.lock().unwrap().insert(id, answer);
    let _ = shared.events.send(Event::JoinRequest {
        id: id.to_string(),
        name: name.into(),
    });
    let allowed = matches!(
        tokio::time::timeout(APPROVAL_TIMEOUT, answered).await,
        Ok(Ok(true))
    );
    shared.approvals.lock().unwrap().remove(&id);
    let _ = shared
        .events
        .send(Event::JoinRequestClosed { id: id.to_string() });

    if allowed {
        shared.store.lock().unwrap().add_friend(Device {
            name: name.into(),
            endpoint_id: id.to_string(),
        })?;
    }
    Ok(allowed)
}

async fn serve(
    conn: &Connection,
    ip: Ipv4Addr,
    room: &str,
    who: &str,
    tun: &AsyncDevice,
    peers: &Peers,
    events: &Events,
) -> Result<()> {
    let mut assign = conn.open_uni().await?;
    assign.write_all(&ip.octets()).await?;
    assign.write_all(link::encode_name(room)).await?;
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
