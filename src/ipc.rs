//! Newline-delimited JSON between the privileged daemon and unprivileged clients (CLI, later GUI).

use std::{io, net::Ipv4Addr};

use interprocess::local_socket::{
    ListenerOptions, Name,
    tokio::{Listener, RecvHalf, SendHalf, Stream, prelude::*},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    Host,
    Join { code: String },
    Leave,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Status {
        version: String,
        in_session: bool,
    },
    Error {
        message: String,
    },
    Hosting {
        code: String,
        ip: Ipv4Addr,
    },
    Joined {
        ip: Ipv4Addr,
    },
    Peer {
        who: String,
        joined: bool,
    },
    Path {
        who: String,
        relayed: bool,
        rtt_ms: u64,
    },
    Ended {
        error: Option<String>,
    },
}

pub type Events = tokio::sync::broadcast::Sender<Event>;

#[cfg(target_os = "macos")]
const SOCKET_PATH: &str = "/var/run/local-mates.sock";
#[cfg(all(unix, not(target_os = "macos")))]
const SOCKET_PATH: &str = "/run/local-mates.sock";

fn name() -> io::Result<Name<'static>> {
    #[cfg(windows)]
    return "local-mates".to_ns_name::<interprocess::local_socket::GenericNamespaced>();
    #[cfg(unix)]
    return SOCKET_PATH.to_fs_name::<interprocess::local_socket::GenericFilePath>();
}

/// Any interactively logged-in user may drive the daemon, as with Tailscale/ZeroTier.
pub fn listen() -> io::Result<Listener> {
    let opts = ListenerOptions::new().name(name()?);

    #[cfg(windows)]
    let opts = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        // SYSTEM and admins: full control; interactive users: read/write.
        let sddl = widestring::u16cstr!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)");
        opts.security_descriptor(SecurityDescriptor::deserialize(sddl)?)
    };
    #[cfg(target_os = "linux")]
    let opts = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        opts.mode(0o666)
    };
    #[cfg(unix)]
    let opts = opts.try_overwrite(true);

    let listener = opts.create_tokio()?;

    // macOS can't set the mode at creation, so open it up afterwards.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(SOCKET_PATH, std::fs::Permissions::from_mode(0o666))?;
    }

    Ok(listener)
}

pub async fn connect() -> io::Result<(Rx, Tx)> {
    Ok(split(Stream::connect(name()?).await?))
}

pub fn split(stream: Stream) -> (Rx, Tx) {
    let (recv, send) = stream.split();
    (Rx(BufReader::new(recv).lines()), Tx(send))
}

pub struct Rx(Lines<BufReader<RecvHalf>>);

impl Rx {
    /// `None` once the other side hangs up. Cancel-safe.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        match self.0.next_line().await? {
            Some(line) => Ok(Some(serde_json::from_str(&line)?)),
            None => Ok(None),
        }
    }
}

pub struct Tx(SendHalf);

impl Tx {
    pub async fn send(&mut self, msg: &impl Serialize) -> io::Result<()> {
        let mut line = serde_json::to_vec(msg)?;
        line.push(b'\n');
        self.0.write_all(&line).await
    }
}
