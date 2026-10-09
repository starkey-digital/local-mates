//! The privileged side: owns the virtual adapter and runs one session at a time for its clients.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use interprocess::local_socket::tokio::{Listener, Stream, prelude::*};
use iroh::EndpointId;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_util::sync::CancellationToken;

use crate::{
    ipc::{self, Event, Events, Request},
    session::{self, Kind},
};

struct Daemon {
    events: Events,
    session: Mutex<Option<CancellationToken>>,
}

pub async fn run(listener: Listener) -> Result<()> {
    let daemon = Arc::new(Daemon {
        events: broadcast::channel(64).0,
        session: Mutex::default(),
    });
    loop {
        let stream = listener.accept().await?;
        let daemon = daemon.clone();
        tokio::spawn(async move {
            if let Err(err) = serve(stream, &daemon).await {
                tracing::debug!("client disconnected: {err}");
            }
        });
    }
}

async fn serve(stream: Stream, daemon: &Arc<Daemon>) -> std::io::Result<()> {
    let (mut rx, mut tx) = ipc::split(stream);
    let mut events = daemon.events.subscribe();
    loop {
        tokio::select! {
            req = rx.recv::<Request>() => match req? {
                Some(req) => if let Some(reply) = daemon.handle(req) {
                    tx.send(&reply).await?;
                },
                None => return Ok(()),
            },
            event = events.recv() => match event {
                Ok(event) => tx.send(&event).await?,
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}

impl Daemon {
    fn handle(self: &Arc<Self>, req: Request) -> Option<Event> {
        let kind = match req {
            Request::Status => {
                return Some(Event::Status {
                    version: env!("CARGO_PKG_VERSION").into(),
                    in_session: self.session.lock().unwrap().is_some(),
                });
            }
            Request::Leave => {
                if let Some(cancel) = &*self.session.lock().unwrap() {
                    cancel.cancel();
                }
                return None;
            }
            Request::Host => Kind::Host,
            Request::Join { code } => match code.trim().parse::<EndpointId>() {
                Ok(host) => Kind::Join(host),
                Err(_) => return Some(error("That code doesn't look right")),
            },
        };
        self.start(kind)
    }

    fn start(self: &Arc<Self>, kind: Kind) -> Option<Event> {
        let cancel = CancellationToken::new();
        {
            let mut session = self.session.lock().unwrap();
            if session.is_some() {
                return Some(error("Already in a session; leave it first"));
            }
            *session = Some(cancel.clone());
        }

        let daemon = self.clone();
        tokio::spawn(async move {
            let res = session::run(kind, daemon.events.clone(), cancel).await;
            // Cleared only once the adapter is gone, so a new session can't race the old one.
            daemon.session.lock().unwrap().take();
            let _ = daemon.events.send(Event::Ended {
                error: res.err().map(|err| format!("{err:#}")),
            });
        });
        None
    }
}

fn error(message: &str) -> Event {
    Event::Error {
        message: message.into(),
    }
}
