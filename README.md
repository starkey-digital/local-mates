# local mates

Dead-simple virtual LAN for playing LAN games with friends. Host a session, share a code, play. No accounts, nothing left behind when the session ends.

Windows-first, also runs on macOS and Linux. Work in progress.

## Prototype usage

Command-line only for now. Needs admin (Windows) or root (macOS/Linux) to create the virtual adapter. On Windows, `wintun.dll` must sit next to `local-mates.exe` (CI builds bundle it).

```sh
local-mates host          # prints a join command with your code
local-mates join <code>   # on your friend's machine
```

The host is `10.77.0.1`; joiners get `10.77.0.2` and up. Each side prints whether the link is direct or relayed.

## How it works

- [iroh](https://iroh.computer) for encrypted peer-to-peer QUIC with NAT hole-punching, falling back to relays.
- [Wintun](https://www.wintun.net) (via `tun-rs`) for the virtual adapter; utun/tun on macOS/Linux.
- The host acts as a hub: joiners' IP packets travel as QUIC datagrams, and broadcast/multicast (how LAN games discover each other) is fanned out to everyone.
- On Windows the adapter gets the lowest route metric so game broadcasts go out of it, and is marked as a Private network so the firewall doesn't block games.
- [Velopack](https://velopack.io) handles install and auto-updates from GitHub Releases: new versions download in the background and install on next launch.

## Relay

`relay/` builds the official [iroh relay](https://crates.io/crates/iroh-relay), and `deploy/relay` is a Helm chart for it. It needs public TCP 443 and UDP 7842 (UDP is how clients learn their public address for hole-punching, so it can't sit behind an HTTP-only proxy).

```sh
helm install relay oci://ghcr.io/starkey-digital/charts/local-mates-relay \
  --set hostname=relay.example.com --set tls.clusterIssuer=letsencrypt
```

## Releasing

Bump `version` in `Cargo.toml`, then push a matching tag (`v0.2.0`). The Release workflow publishes the Windows installer to GitHub Releases (installed copies pick it up), plus the relay image and chart to GHCR.
