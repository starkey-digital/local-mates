//! Client for the rooms server, which turns short codes into endpoint ids for online rooms.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::EndpointId;
use mates_proto::{Code, RENEW_SECS, Room};
use reqwest::StatusCode;

const DEFAULT_API: &str = "https://mates.jakestarkey.dev";

fn url(code: &Code) -> String {
    let base = std::env::var("LOCAL_MATES_API").unwrap_or_else(|_| DEFAULT_API.into());
    format!("{base}/v1/rooms/{}", code.as_str())
}

pub fn client() -> reqwest::Client {
    // reqwest is built without a bundled crypto provider; use ring, which iroh already ships.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("TLS provider is installed")
}

pub fn code_for(id: EndpointId) -> Code {
    Code::for_endpoint(id.as_bytes())
}

pub async fn resolve(http: &reqwest::Client, code: &Code) -> Result<EndpointId> {
    let res = http
        .get(url(code))
        .send()
        .await
        .context("couldn't reach the local mates server to look up that code")?;
    match res.status() {
        StatusCode::NOT_FOUND => bail!("no room with code {code} is online right now"),
        StatusCode::TOO_MANY_REQUESTS => bail!("too many attempts; wait a minute and try again"),
        _ => {}
    }
    let room: Room = res.error_for_status()?.json().await?;
    room.endpoint_id
        .parse()
        .context("the server returned a malformed room")
}

/// Keeps this room listed while hosting; it drops off the server a few minutes after this stops.
pub async fn announce(http: reqwest::Client, id: EndpointId) {
    let (code, room) = (
        code_for(id),
        Room {
            endpoint_id: id.to_string(),
        },
    );
    let mut tick = tokio::time::interval(Duration::from_secs(RENEW_SECS));
    loop {
        tick.tick().await;
        match http.put(url(&code)).json(&room).send().await {
            Ok(res) if res.status() == StatusCode::CONFLICT => {
                tracing::warn!("code {code} is taken by another room; friends need the long code");
            }
            Ok(res) if !res.status().is_success() => {
                tracing::warn!("couldn't list room: {}", res.status());
            }
            Err(err) => tracing::warn!("couldn't reach the local mates server: {err}"),
            Ok(_) => {}
        }
    }
}
