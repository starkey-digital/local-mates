use std::{convert::Infallible, net::Ipv4Addr};

use anyhow::{Context, Result};
use bytes::Bytes;
use iroh::{Endpoint, EndpointId, endpoint::Connection};
use tun_rs::AsyncDevice;

use crate::{link, packet::HOST_IP, tun};

pub async fn run(endpoint: Endpoint, host: EndpointId) -> Result<()> {
    println!("Connecting to host...");
    let conn = endpoint
        .connect(host, link::ALPN)
        .await
        .context("couldn't reach the host: check the code and that they're still hosting")?;

    let assigned: [u8; 4] = conn
        .accept_uni()
        .await?
        .read_to_end(4)
        .await?
        .try_into()
        .map_err(|_| anyhow::anyhow!("host sent a malformed address"))?;
    let ip = Ipv4Addr::from(assigned);

    let tun = tun::open(ip)?;
    println!("Joined as {ip}. The host is {HOST_IP}.");
    tokio::spawn(link::report_path(conn.clone(), "host".into()));

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
