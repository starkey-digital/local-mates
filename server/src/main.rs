//! Tracks which rooms are online so a short code can be turned into the host's endpoint id.
//! Everything is in memory: hosts re-announce every minute, so a restart heals itself.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderName, StatusCode},
    routing::get,
};
use mates_proto::{Code, RENEW_SECS, Room, TTL_SECS, parse_endpoint_id};

const MAX_ROOMS: usize = 100_000;
/// Per client IP; generous for real use (a renew a minute), hopeless for guessing codes.
const REQUESTS_PER_MINUTE: u32 = 30;

struct App {
    rooms: Mutex<HashMap<Code, (String, Instant)>>,
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    /// Set when behind a proxy (e.g. `CF-Connecting-IP` for Cloudflare) so limits apply per
    /// real client rather than per proxy.
    client_ip_header: Option<HeaderName>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    let app = Arc::new(App {
        rooms: Mutex::default(),
        hits: Mutex::default(),
        client_ip_header: std::env::var("CLIENT_IP_HEADER")
            .ok()
            .map(|h| h.parse().expect("CLIENT_IP_HEADER must be a header name")),
    });
    tokio::spawn(sweep(app.clone()));

    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/rooms/{code}", get(lookup).put(announce))
        .with_state(app);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("[::]:{port}")).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

async fn lookup(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<Room>, StatusCode> {
    app.limit(&headers, peer)?;
    let code = Code::parse(&code).ok_or(StatusCode::BAD_REQUEST)?;
    match app.rooms.lock().unwrap().get(&code) {
        Some((endpoint_id, expires)) if *expires > Instant::now() => Ok(Json(Room {
            endpoint_id: endpoint_id.clone(),
        })),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

/// No auth needed: the code must be derived from the endpoint id, so announcing only ever
/// says "this endpoint is online", which anyone can learn from iroh's discovery anyway.
async fn announce(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(room): Json<Room>,
) -> StatusCode {
    if let Err(status) = app.limit(&headers, peer) {
        return status;
    }
    let (Some(code), Some(id)) = (Code::parse(&code), parse_endpoint_id(&room.endpoint_id)) else {
        return StatusCode::BAD_REQUEST;
    };
    if Code::for_endpoint(&id) != code {
        return StatusCode::BAD_REQUEST;
    }

    let now = Instant::now();
    let mut rooms = app.rooms.lock().unwrap();
    match rooms.get(&code) {
        // Two rooms whose ids hash to the same code: first one online keeps it.
        Some((other, expires)) if *other != room.endpoint_id && *expires > now => {
            return StatusCode::CONFLICT;
        }
        None if rooms.len() >= MAX_ROOMS => return StatusCode::SERVICE_UNAVAILABLE,
        _ => {}
    }
    let expires = now + Duration::from_secs(TTL_SECS);
    rooms.insert(code, (room.endpoint_id, expires));
    StatusCode::NO_CONTENT
}

impl App {
    fn limit(&self, headers: &HeaderMap, peer: SocketAddr) -> Result<(), StatusCode> {
        let ip = self
            .client_ip_header
            .as_ref()
            .and_then(|h| headers.get(h)?.to_str().ok()?.trim().parse().ok())
            .unwrap_or(peer.ip());

        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap();
        let (window, count) = hits.entry(ip).or_insert((now, 0));
        if now.duration_since(*window) >= Duration::from_secs(60) {
            (*window, *count) = (now, 0);
        }
        *count += 1;
        if *count > REQUESTS_PER_MINUTE {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        Ok(())
    }
}

async fn sweep(app: Arc<App>) {
    let mut tick = tokio::time::interval(Duration::from_secs(RENEW_SECS));
    loop {
        tick.tick().await;
        let now = Instant::now();
        app.rooms
            .lock()
            .unwrap()
            .retain(|_, (_, expires)| *expires > now);
        app.hits
            .lock()
            .unwrap()
            .retain(|_, (window, _)| now.duration_since(*window) < Duration::from_secs(60));
    }
}
