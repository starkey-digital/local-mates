use anyhow::Result;
use iroh::{Endpoint, EndpointId, endpoint::presets};
use tokio_util::sync::CancellationToken;

use crate::{host, ipc::Events, join, link};

pub enum Kind {
    Host,
    Join(EndpointId),
}

pub async fn run(kind: Kind, events: Events, cancel: CancellationToken) -> Result<()> {
    // Fresh identity every session: sessions are throwaway, nothing to remember.
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![link::ALPN.to_vec()])
        .bind()
        .await?;

    let session = async {
        endpoint.online().await;
        match kind {
            Kind::Host => host::run(endpoint.clone(), events).await,
            Kind::Join(host) => join::run(endpoint.clone(), host, events).await,
        }
    };
    let res = cancel.run_until_cancelled(session).await.unwrap_or(Ok(()));

    endpoint.close().await;
    res
}
