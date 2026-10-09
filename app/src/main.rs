//! The Local Mates window and tray icon. A plain client of the background service, like the
//! CLI: quitting leaves any session running. Closing the window only hides it to the tray.

// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod service;

use std::{rc::Rc, time::Duration};

use local_mates::ipc::{self, Event, Request};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};
use tokio::sync::mpsc;

slint::include_modules!();

const ACTIVITY_LINES: usize = 6;

fn main() -> anyhow::Result<()> {
    // Must run first: Velopack may handle install/update hooks here and exit or restart.
    velopack::VelopackApp::build().run();
    local_mates::update::spawn_check();

    let ui = AppWindow::new()?;
    ui.set_can_setup(service::CAN_SETUP);

    let (commands, queued) = mpsc::unbounded_channel();
    wire_callbacks(&ui, commands);

    // Visible from creation; it keeps the event loop alive while the window is hidden.
    let tray = Tray::new()?;
    let weak = ui.as_weak();
    tray.on_open(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.show();
        }
    });
    tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.spawn(connection(ui.as_weak(), queued));

    ui.show()?;
    slint::run_event_loop_until_quit()?;
    drop(tray);
    Ok(())
}

fn wire_callbacks(ui: &AppWindow, commands: mpsc::UnboundedSender<Request>) {
    let send = Rc::new(move |req: Request| {
        let _ = commands.send(req);
    });

    // Optimistic: show progress straight away; the service's events then take over.
    let (s, weak) = (send.clone(), ui.as_weak());
    ui.on_host(move || {
        start(&weak);
        s(Request::Host);
    });
    let (s, weak) = (send.clone(), ui.as_weak());
    ui.on_join(move |code| {
        start(&weak);
        s(Request::Join { code: code.into() });
    });
    let (s, weak) = (send.clone(), ui.as_weak());
    ui.on_join_saved(move |name| {
        start(&weak);
        s(Request::Join { code: name.into() });
    });

    let s = send.clone();
    ui.on_leave(move || s(Request::Leave));
    let s = send.clone();
    ui.on_forget(move |name| s(Request::Forget { name: name.into() }));
    let (s, weak) = (send, ui.as_weak());
    ui.on_answer(move |id, allow| {
        if let Some(ui) = weak.upgrade() {
            remove_request(&ui, &id);
        }
        s(Request::Approve {
            id: id.into(),
            allow,
        });
    });

    ui.on_copy(|text| {
        if let Err(err) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.as_str())) {
            eprintln!("couldn't copy: {err}");
        }
    });

    let weak = ui.as_weak();
    ui.on_setup(move || {
        let Some(ui) = weak.upgrade() else { return };
        ui.set_setting_up(true);
        ui.set_error(SharedString::new());
        let weak = weak.clone();
        // Blocks on the UAC prompt and install, so off the UI thread.
        std::thread::spawn(move || {
            let res = service::install();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_setting_up(false);
                if let Err(err) = res {
                    ui.set_error(format!("{err:#}").into());
                }
            });
        });
    });
}

fn start(ui: &Weak<AppWindow>) {
    if let Some(ui) = ui.upgrade() {
        ui.set_phase(Phase::Starting);
        ui.set_error(SharedString::new());
    }
}

/// Keeps a connection to the service, reconnecting if it restarts or isn't installed yet.
async fn connection(ui: Weak<AppWindow>, mut commands: mpsc::UnboundedReceiver<Request>) {
    loop {
        if let Ok((mut rx, mut tx)) = ipc::connect().await {
            // Drop anything clicked while offline; it was against stale state.
            while commands.try_recv().is_ok() {}
            let greeted = async {
                tx.send(&Request::Status).await?;
                tx.send(&Request::Rooms).await
            };
            if greeted.await.is_ok() {
                loop {
                    tokio::select! {
                        event = rx.recv::<Event>() => {
                            let Ok(Some(event)) = event else { break };
                            // Joining saves the room, and errors may leave the UI's phase
                            // wrong; ask for fresh state.
                            let refresh = match event {
                                Event::Joined { .. } => Some(Request::Rooms),
                                Event::Error { .. } => Some(Request::Status),
                                _ => None,
                            };
                            let _ = ui.upgrade_in_event_loop(move |ui| apply(&ui, event));
                            if let Some(req) = refresh
                                && tx.send(&req).await.is_err()
                            {
                                break;
                            }
                        }
                        req = commands.recv() => {
                            let Some(req) = req else { return };
                            if tx.send(&req).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        let _ = ui.upgrade_in_event_loop(|ui| ui.set_phase(Phase::Offline));
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn apply(ui: &AppWindow, event: Event) {
    match event {
        Event::Status { in_session, .. } => {
            // A session in progress reports its own state right after this.
            ui.set_phase(if in_session {
                Phase::Starting
            } else {
                Phase::Idle
            });
        }
        Event::Rooms { my_code, rooms, .. } => {
            ui.set_my_code(my_code.into());
            let rooms: Vec<Room> = rooms
                .into_iter()
                .map(|r| Room {
                    name: r.name.into(),
                })
                .collect();
            ui.set_rooms(ModelRc::new(VecModel::from(rooms)));
        }
        Event::Hosting {
            code,
            long_code,
            ip,
        } => {
            ui.set_phase(Phase::Hosting);
            ui.set_my_code(code.into());
            ui.set_long_code(long_code.into());
            ui.set_my_ip(ip.to_string().into());
        }
        Event::Waiting => ui.set_phase(Phase::Waiting),
        Event::Joined { room, ip } => {
            ui.set_phase(Phase::Joined);
            ui.set_room_name(room.into());
            ui.set_my_ip(ip.to_string().into());
        }
        Event::Peer { who, joined } => log(
            ui,
            format!("{who} {}", if joined { "joined" } else { "left" }),
        ),
        Event::Path {
            who,
            relayed,
            rtt_ms,
        } => log(
            ui,
            format!(
                "{who}: {} ({rtt_ms} ms)",
                if relayed { "relayed" } else { "direct" }
            ),
        ),
        Event::JoinRequest { id, name } => {
            let mut requests: Vec<JoinRequest> = ui.get_requests().iter().collect();
            if !requests.iter().any(|r| r.id == id) {
                requests.push(JoinRequest {
                    id: id.into(),
                    name: name.into(),
                });
                ui.set_requests(ModelRc::new(VecModel::from(requests)));
                // Someone's waiting on an answer: bring the window back from the tray.
                let _ = ui.show();
            }
        }
        Event::JoinRequestClosed { id } => remove_request(ui, &id),
        Event::Ended { error } => {
            ui.set_phase(Phase::Idle);
            ui.set_requests(ModelRc::default());
            ui.set_error(error.unwrap_or_default().into());
        }
        Event::Error { message } => ui.set_error(message.into()),
    }
}

fn remove_request(ui: &AppWindow, id: &str) {
    let requests: Vec<JoinRequest> = ui.get_requests().iter().filter(|r| r.id != id).collect();
    ui.set_requests(ModelRc::new(VecModel::from(requests)));
}

fn log(ui: &AppWindow, line: String) {
    let mut lines: Vec<SharedString> = ui.get_activity().iter().collect();
    lines.push(line.into());
    let excess = lines.len().saturating_sub(ACTIVITY_LINES);
    lines.drain(..excess);
    ui.set_activity(ModelRc::new(VecModel::from(lines)));
}
