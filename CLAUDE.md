# Local Mates

Public repo: keep infrastructure details (IPs, cluster layout, secrets, values) out of it. Deployment values live in the private GitOps repo.

## Layout

Cargo workspace:

- `src/` — core library plus the `local-mates` binary (CLI and the privileged daemon/Windows service). Clients talk to the daemon over a local socket (`ipc.rs`, newline-delimited JSON).
- `app/` — `local-mates-app`, the Slint desktop app and tray. A plain IPC client like the CLI; it never touches the adapter. UI lives in `app/ui/app.slint`.
- `server/` — `mates-server`, the rooms service that maps short codes to online hosts.
- `proto/` — code format and rooms API types shared by app and server.
- `relay/` + `deploy/local-mates/` — relay image and the Helm chart for the server side.

## Gotchas

- Room codes are derived from the device key (`proto::Code::for_endpoint`). `code_derivation_is_frozen` pins it: changing the derivation silently changes every user's code.
- Changing anything in `ipc::Request`/`Event` or the wire handshake (`host.rs`/`join.rs`) affects both the CLI and the app, and old services still talk to new clients. The client warns on version mismatch; keep that path working.
- Windows: tun-rs `mtu()` also sets the IPv6 MTU, which Windows rejects below 1280. Only set `mtu_v4` there.
- The daemon runs as SYSTEM on Windows, so `service install` copies the exe into Program Files. Never point the service at the per-user Velopack directory.
- Linux builds of `app` need `libfontconfig-dev` and `libxkbcommon-dev`.

## Testing

- `cargo test --workspace` runs `src/net_tests.rs`: real iroh endpoints over localhost with an in-memory adapter, so no admin or network setup. Add networking behaviour there.
- CI's Windows job (`.github/scripts/windows-smoke.ps1`) installs the service and checks the real Wintun adapter. It's the only coverage of service install and adapter settings.
- Windows cross-check from macOS: `PATH="/opt/homebrew/opt/llvm/bin:$PATH" cargo xwin clippy --target x86_64-pc-windows-msvc -p local-mates -p local-mates-app --all-targets -- -D warnings` (needs `cargo install cargo-xwin` and `brew install llvm lld`).
- UI changes: `cargo run -p local-mates-app --example preview -- <dir>` renders each UI state offscreen to PPM files (`sips -s format png` converts them). Add a state there when adding one to the UI.
- Low on disk: build with `CARGO_PROFILE_DEV_DEBUG=0`. The workspace's debug target is several GB otherwise.

## Release

- Bump `version` in `Cargo.toml` (and `cargo update -p local-mates` for the lockfile) in the release-notes commit; the Release workflow fails if the tag doesn't match.
- Tags are `v`-prefixed (`v0.2.0`).
- Pushing the tag runs `.github/workflows/release.yml`, which creates the GitHub release itself (via `vpk upload`) and publishes the server images and Helm chart to GHCR. Do **not** run `gh release create`; once the workflow finishes, attach the notes with `gh release edit <tag> --notes-file changelog/RELEASE_<version>.md`.
- Link issues/PRs in changelog entries.
