//! The local mates core, shared by the CLI/service (`local-mates`) and the desktop app.

pub mod client;
pub mod daemon;
mod host;
pub mod ipc;
mod join;
mod link;
#[cfg(test)]
mod net_tests;
mod packet;
mod rooms_api;
#[cfg(windows)]
pub mod service;
mod session;
pub mod store;
mod tun;
pub mod update;
