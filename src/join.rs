use std::{convert::Infallible, net::Ipv4Addr};

use anyhow::{Context, Result};
use bytes::Bytes;
use iroh::{Endpoint, EndpointId, endpoint::Connection};
use tun_rs::AsyncDevice;

use crate::{ipc::Event, link, session::Shared, store::Device, tun};

pub async fn run(endpoint: Endpoint, host: EndpointId, shared: &Shared) -> Result<()> {
    let conn = endpoint
        .connect(host, link::ALPN)
        .await
        .context("couldn't reach the room: the host may not be hosting right now")?;

    let mut hello = conn.open_uni().await?;
    hello
        .write_all(link::encode_name(&link::device_name()))
        .await?;
    hello.finish()?;
    let _ = shared.events.send(Event::Waiting);

    // Arrives once the host lets us in; a refusal closes the connection instead.
    let mut welcome = conn.accept_uni().await.map_err(link::explain_close)?;
    let welcome = welcome.read_to_end(4 + link::MAX_NAME).await?;
    let (ip, name) = welcome
        .split_first_chunk::<4>()
        .context("host sent a malformed greeting")?;
    let ip = Ipv4Addr::from(*ip);
    let name = link::decode_name(name, "Unnamed room");

    let tun = tun::open(ip)?;
    shared.store.lock().unwrap().remember_room(Device {
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
