use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use iroh::{Endpoint, EndpointId, endpoint::presets};
use mates_proto::Code;
use tokio_util::sync::CancellationToken;

use crate::{host, ipc::Events, join, link, rooms_api, store::Store};

pub enum Kind {
    Host,
    /// Short code, long code, or a saved room's name.
    Join(String),
}

/// What sessions share with the daemon.
#[derive(Clone)]
pub struct Shared {
    pub events: Events,
    pub store: Arc<Mutex<Store>>,
    pub http: reqwest::Client,
}

pub async fn run(kind: Kind, shared: &Shared, cancel: CancellationToken) -> Result<()> {
    // One persistent identity per device: it's this device's room, and lets hosts recognise
    // returning friends.
    let key = shared.store.lock().unwrap().key()?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(key)
        .alpns(vec![link::ALPN.to_vec()])
        .bind()
        .await?;

    let session = async {
        match kind {
            Kind::Host => {
                endpoint.online().await;
                host::run(endpoint.clone(), shared).await
            }
            Kind::Join(target) => {
                let host = resolve(&target, shared).await?;
                endpoint.online().await;
                join::run(endpoint.clone(), host, shared).await
            }
        }
    };
    let res = cancel.run_until_cancelled(session).await.unwrap_or(Ok(()));

    endpoint.close().await;
    res
}

async fn resolve(target: &str, shared: &Shared) -> Result<EndpointId> {
    let saved = shared.store.lock().unwrap().find(target).cloned();
    if let Some(room) = saved {
        return room.endpoint_id.parse().context("saved room is corrupt");
    }
    if let Some(code) = Code::parse(target) {
        return rooms_api::resolve(&shared.http, &code).await;
    }
    target
        .trim()
        .parse()
        .ok()
        .context("that's not a room code or the name of a saved room")
}
