//! The privileged side: owns the virtual adapter and runs one session at a time for its clients.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use interprocess::local_socket::tokio::{Listener, Stream, prelude::*};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_util::sync::CancellationToken;

use crate::{
    ipc::{self, Event, Request},
    rooms_api,
    session::{self, Kind, Shared},
    store::Store,
};

struct Daemon {
    shared: Shared,
    session: Mutex<Option<CancellationToken>>,
}

pub async fn run(listener: Listener) -> Result<()> {
    let daemon = Arc::new(Daemon {
        shared: Shared {
            events: broadcast::channel(64).0,
            store: Arc::new(Mutex::new(Store::load())),
            rooms_api: Some(rooms_api::RoomsApi::from_env()),
            approvals: Arc::default(),
        },
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
    let mut events = daemon.shared.events.subscribe();
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
            Request::Rooms => return Some(self.rooms()),
            Request::Forget { name } => {
                return Some(match self.shared.store.lock().unwrap().forget(&name) {
                    Ok(true) => self.rooms(),
                    Ok(false) => error(&format!("No saved room called {name}")),
                    Err(err) => error(&format!("Couldn't save: {err}")),
                });
            }
            Request::ResetCode => {
                // Holding the session lock stops a session starting with the old key meanwhile.
                let session = self.session.lock().unwrap();
                if session.is_some() {
                    return Some(error("Leave the current session first"));
                }
                if let Err(err) = self.shared.store.lock().unwrap().reset_key() {
                    return Some(error(&format!("Couldn't save: {err}")));
                }
                drop(session);
                return Some(self.rooms());
            }
            Request::Approve { id, allow } => {
                let pending = id
                    .parse::<iroh::EndpointId>()
                    .ok()
                    .and_then(|id| self.shared.approvals.lock().unwrap().remove(&id));
                if let Some(answer) = pending {
                    let _ = answer.send(allow);
                }
                return None;
            }
            Request::Host => Kind::Host,
            Request::Join { code } => Kind::Join(code),
        };
        self.start(kind)
    }

    fn rooms(&self) -> Event {
        let mut store = self.shared.store.lock().unwrap();
        match store.key() {
            Ok(key) => Event::Rooms {
                my_code: rooms_api::code_for(key.public()).to_string(),
                rooms: store.rooms().to_vec(),
                friends: store.friends().to_vec(),
            },
            Err(err) => error(&format!("Couldn't save: {err}")),
        }
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
            let res = session::run(kind, &daemon.shared, cancel).await;
            // Cleared only once the adapter is gone, so a new session can't race the old one.
            daemon.session.lock().unwrap().take();
            let _ = daemon.shared.events.send(Event::Ended {
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
