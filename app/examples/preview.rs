//! Renders the window in each state to images, without a display or the service:
//!     cargo run -p local-mates-app --example preview -- <output-dir>
//! Writes one binary PPM per state (convert with e.g. `sips -s format png` or ImageMagick).

use std::{fs, io::Write, path::Path, rc::Rc};

use slint::{
    ComponentHandle, ModelRc, PhysicalSize, SharedString, VecModel,
    platform::{
        Platform, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType},
    },
};

slint::include_modules!();

const SIZE: (u32, u32) = (380, 640);

type State = (&'static str, fn(&AppWindow));

struct Offscreen(Rc<MinimalSoftwareWindow>);

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: preview <output-dir>");
    fs::create_dir_all(&out).unwrap();

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen(window.clone()))).unwrap();
    window.set_size(PhysicalSize::new(SIZE.0, SIZE.1));

    let states: [State; 6] = [
        ("1-setup", |ui| {
            ui.set_phase(Phase::Offline);
        }),
        ("2-idle", |ui| {
            ui.set_phase(Phase::Idle);
            ui.set_rooms(model(vec![
                Room {
                    name: "Sam's room".into(),
                },
                Room {
                    name: "LAN party at Alex's".into(),
                },
            ]));
        }),
        ("3-hosting-request", |ui| {
            ui.set_phase(Phase::Hosting);
            ui.set_my_ip("10.77.0.1".into());
            ui.set_requests(model(vec![JoinRequest {
                id: "x".into(),
                name: "DESKTOP-SAM".into(),
            }]));
            ui.set_activity(model(vec![
                SharedString::from("ALEX-PC (10.77.0.2) joined"),
                SharedString::from("ALEX-PC (10.77.0.2): direct (14 ms)"),
            ]));
        }),
        ("4-waiting", |ui| ui.set_phase(Phase::Waiting)),
        ("5-joined", |ui| {
            ui.set_phase(Phase::Joined);
            ui.set_room_name("Sam's room".into());
            ui.set_my_ip("10.77.0.3".into());
        }),
        ("6-error", |ui| {
            ui.set_phase(Phase::Idle);
            ui.set_error("No room with code K7M-Q2X is online right now".into());
        }),
    ];

    for (name, setup) in states {
        let ui = AppWindow::new().unwrap();
        ui.set_my_code("K7M-Q2X".into());
        setup(&ui);
        ui.show().unwrap();
        slint::platform::update_timers_and_animations();
        render(&window, &Path::new(&out).join(format!("{name}.ppm")));
        ui.hide().unwrap();
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn render(window: &MinimalSoftwareWindow, path: &Path) {
    let (w, h) = (SIZE.0 as usize, SIZE.1 as usize);
    let mut buf = vec![PremultipliedRgbaColor::default(); w * h];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buf, w);
    });
    let mut file = fs::File::create(path).unwrap();
    write!(file, "P6\n{w} {h}\n255\n").unwrap();
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue]).collect();
    file.write_all(&rgb).unwrap();
}
