use std::{collections::VecDeque, io::Write};

use anyhow::{Context, Result, anyhow, bail};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
    daemon,
    ipc::{self, Event, Request, Rx, Tx},
};

/// Starts a session and prints its progress until it ends; Ctrl-C leaves. While hosting, asks
/// on the terminal whether to let new people in.
pub async fn session(req: Request) -> Result<()> {
    let (mut rx, mut tx) = connect_or_spawn().await?;
    check_version(&mut rx, &mut tx).await?;
    tx.send(&req).await?;

    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdin_open = true;
    // (id, name) of join requests, asked about one at a time.
    let mut asking: VecDeque<(String, String)> = VecDeque::new();

    loop {
        tokio::select! {
            event = rx.recv::<Event>() => match event?.context("lost connection to the local mates service")? {
                Event::Ended { error } => return error.map_or(Ok(()), |err| Err(anyhow!(err))),
                Event::Error { message } => bail!(message),
                Event::JoinRequest { id, name } => {
                    asking.push_back((id, name));
                    if asking.len() == 1 {
                        ask(&asking[0].1);
                    }
                }
                // Answered elsewhere (e.g. another window) or timed out.
                Event::JoinRequestClosed { id } => {
                    let was_asking = asking.front().is_some_and(|(asked, _)| *asked == id);
                    asking.retain(|(asked, _)| *asked != id);
                    if was_asking {
                        println!();
                        if let Some((_, name)) = asking.front() {
                            ask(name);
                        }
                    }
                }
                event => print(event),
            },
            line = stdin.next_line(), if stdin_open && !asking.is_empty() => {
                let Some(line) = line? else {
                    stdin_open = false;
                    continue;
                };
                let (id, _) = asking.pop_front().expect("guarded by !is_empty");
                let allow = matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
                tx.send(&Request::Approve { id, allow }).await?;
                if let Some((_, name)) = asking.front() {
                    ask(name);
                }
            }
            _ = tokio::signal::ctrl_c() => tx.send(&Request::Leave).await?,
        }
    }
}

fn ask(name: &str) {
    print!("{name} wants to join your room. Let them in? [y/N] ");
    let _ = std::io::stdout().flush();
}

/// For `Rooms`, `Forget` and `ResetCode`, which all reply with the room list.
pub async fn rooms(req: Request) -> Result<()> {
    let (mut rx, mut tx) = connect_or_spawn().await?;
    tx.send(&req).await?;
    loop {
        match rx.recv().await?.context("no reply from service")? {
            Event::Rooms {
                my_code,
                rooms,
                friends,
            } => {
                println!("Your room's code: {my_code}");
                if rooms.is_empty() {
                    println!("\nNo saved rooms yet. Rooms you join are saved here.");
                } else {
                    println!("\nSaved rooms (reconnect with `local-mates join \"<name>\"`):");
                    for room in rooms {
                        println!("    {}", room.name);
                    }
                }
                if !friends.is_empty() {
                    println!("\nAllowed into your room without asking:");
                    for friend in friends {
                        println!("    {}", friend.name);
                    }
                }
                println!("\nRemove either with `local-mates forget \"<name>\"`.");
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
        Event::Waiting => println!("Waiting for the host to let you in..."),
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
        Event::Status { .. }
        | Event::Error { .. }
        | Event::Ended { .. }
        | Event::Rooms { .. }
        | Event::JoinRequest { .. }
        | Event::JoinRequestClosed { .. } => {}
    }
}
