use std::net::Ipv4Addr;

use anyhow::{Context, Result};
use tun_rs::{AsyncDevice, DeviceBuilder};

use crate::packet::PREFIX;

/// Below iroh's guaranteed minimum datagram size, so every packet fits in one datagram.
pub const MTU: u16 = 1100;

#[cfg(windows)]
const ADAPTER_NAME: &str = "local mates";

pub fn open(ip: Ipv4Addr) -> Result<AsyncDevice> {
    let builder = DeviceBuilder::new().ipv4(ip, PREFIX, None).mtu(MTU);
    // Lowest metric so Windows sends games' 255.255.255.255 discovery broadcasts out of this
    // adapter instead of the real NIC — the classic "connected but can't see the server" bug.
    #[cfg(windows)]
    let builder = builder.name(ADAPTER_NAME).metric(1);
    #[cfg(target_os = "linux")]
    let builder = builder.name("lmates0");

    let dev = builder.build_async().context(
        "couldn't create the virtual network adapter: install the service with \
         `local-mates service install` from an admin terminal on Windows, or run \
         `sudo local-mates daemon` on macOS/Linux",
    )?;

    #[cfg(windows)]
    mark_private();

    Ok(dev)
}

/// New adapters land on the Public firewall profile, which silently blocks game traffic.
/// Windows takes a few seconds to create the connection profile, so retry in the background.
#[cfg(windows)]
fn mark_private() {
    use std::{process::Command, thread, time::Duration};

    thread::spawn(|| {
        let cmd = format!(
            "Set-NetConnectionProfile -InterfaceAlias '{ADAPTER_NAME}' -NetworkCategory Private"
        );
        for _ in 0..15 {
            let ok = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &cmd])
                .output()
                .is_ok_and(|o| o.status.success());
            if ok {
                tracing::info!("adapter set to Private network");
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
        tracing::warn!("couldn't set adapter to Private network; Windows Firewall may block games");
    });
}
