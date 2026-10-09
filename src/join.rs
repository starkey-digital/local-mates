use std::{convert::Infallible, net::Ipv4Addr};

use anyhow::{Context, Result};
use bytes::Bytes;
use iroh::{Endpoint, EndpointId, endpoint::Connection};
use tun_rs::AsyncDevice;

use crate::{ipc::Event, link, session::Shared, store::SavedRoom, tun};

pub const MAX_NAME: usize = 64;

pub async fn run(endpoint: Endpoint, host: EndpointId, shared: &Shared) -> Result<()> {
    let conn = endpoint
        .connect(host, link::ALPN)
        .await
        .context("couldn't reach the room: the host may not be hosting right now")?;

    let hello = conn.accept_uni().await?.read_to_end(4 + MAX_NAME).await?;
    let (ip, name) = hello
        .split_first_chunk::<4>()
        .context("host sent a malformed greeting")?;
    let ip = Ipv4Addr::from(*ip);
    let name = String::from_utf8_lossy(name).trim().to_owned();
    let name = if name.is_empty() {
        "Unnamed room".into()
    } else {
        name
    };

    let tun = tun::open(ip)?;
    shared.store.lock().unwrap().remember(SavedRoom {
        name: name.clone(),
        endpoint_id: host.to_string(),
    })?;
    let _ = shared.events.send(Event::Joined { room: name, ip });
    tokio::spawn(link::report_path(
        conn.clone(),
        "host".into(),
        shared.events.clone(),
    ));

    tokio::try_join!(adapter_to_host(&tun, &conn), host_to_adapter(&tun, &conn))
        .map(|_| ())
        .context("disconnected from host")
}

async fn adapter_to_host(tun: &AsyncDevice, conn: &Connection) -> Result<Infallible> {
    let mut buf = vec![0; tun::MTU as usize];
    loop {
        let n = tun.recv(&mut buf).await?;
        link::send(conn, Bytes::copy_from_slice(&buf[..n]));
    }
}

async fn host_to_adapter(tun: &AsyncDevice, conn: &Connection) -> Result<Infallible> {
    loop {
        let pkt = conn.read_datagram().await?;
        tun.send(&pkt).await?;
    }
}
