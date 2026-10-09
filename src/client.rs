use anyhow::{Context, Result, anyhow, bail};

use crate::{
    daemon,
    ipc::{self, Event, Request, Rx, Tx},
};

/// Starts a session and prints its progress until it ends; Ctrl-C leaves.
pub async fn session(req: Request) -> Result<()> {
    let (mut rx, mut tx) = connect_or_spawn().await?;
    check_version(&mut rx, &mut tx).await?;
    tx.send(&req).await?;

    loop {
        tokio::select! {
            event = rx.recv::<Event>() => match event?.context("lost connection to the local mates service")? {
                Event::Ended { error } => return error.map_or(Ok(()), |err| Err(anyhow!(err))),
                Event::Error { message } => bail!(message),
                event => print(event),
            },
            _ = tokio::signal::ctrl_c() => tx.send(&Request::Leave).await?,
        }
    }
}

/// For `Rooms`, `Forget` and `ResetCode`, which all reply with the room list.
pub async fn rooms(req: Request) -> Result<()> {
    let (mut rx, mut tx) = connect_or_spawn().await?;
    tx.send(&req).await?;
    loop {
        match rx.recv().await?.context("no reply from service")? {
            Event::Rooms { my_code, rooms } => {
                println!("Your room's code: {my_code}\n");
                if rooms.is_empty() {
                    println!("No saved rooms yet. Rooms you join are saved here.");
                    return Ok(());
                }
                for room in rooms {
                    println!("    {}", room.name);
                }
                println!("\nReconnect with `local-mates join \"<name>\"`.");
                return Ok(());
            }
            Event::Error { message } => bail!(message),
            _ => {}
        }
    }
}

pub async fn leave() -> Result<()> {
    let (_, mut tx) = ipc::connect().await.context("local mates isn't running")?;
    Ok(tx.send(&Request::Leave).await?)
}

/// Uses the installed service if there is one, otherwise runs the daemon in this process
/// (which then needs admin/root itself to create the adapter).
async fn connect_or_spawn() -> Result<(Rx, Tx)> {
    if let Ok(conn) = ipc::connect().await {
        return Ok(conn);
    }
    let listener = ipc::listen().context(NO_SERVICE)?;
    tokio::spawn(daemon::run(listener));
    Ok(ipc::connect().await?)
}

#[cfg(windows)]
const NO_SERVICE: &str =
    "local mates service isn't running: run `local-mates service install` from an admin terminal";
#[cfg(not(windows))]
const NO_SERVICE: &str =
    "local mates daemon isn't running: start it with `sudo local-mates daemon`";

async fn check_version(rx: &mut Rx, tx: &mut Tx) -> Result<()> {
    tx.send(&Request::Status).await?;
    loop {
        if let Event::Status { version, .. } = rx.recv().await?.context("no reply from service")? {
            if version != env!("CARGO_PKG_VERSION") {
                eprintln!(
                    "Note: the service is v{version} but this is v{}. Update it with \
                     `local-mates service install` from an admin terminal.",
                    env!("CARGO_PKG_VERSION")
                );
            }
            return Ok(());
        }
    }
}

fn print(event: Event) {
    match event {
        Event::Hosting {
            code,
            long_code,
            ip,
        } => println!(
            "Your room is open on {ip}. Friends join with:\n\n    local-mates join {code}\n\n\
             If the code doesn't work, use:\n\n    local-mates join {long_code}\n"
        ),
        Event::Joined { room, ip } => println!(
            "Joined {room} as {ip}. The host is {}.",
            crate::packet::HOST_IP
        ),
        Event::Peer { who, joined: true } => println!("{who} joined"),
        Event::Peer { who, joined: false } => println!("{who} left"),
        Event::Path {
            who,
            relayed,
            rtt_ms,
        } => println!(
            "{who}: {} ({rtt_ms}ms)",
            if relayed { "relayed" } else { "direct" }
        ),
        Event::Status { .. } | Event::Error { .. } | Event::Ended { .. } | Event::Rooms { .. } => {}
    }
}
