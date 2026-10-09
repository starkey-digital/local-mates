fn main() {
    slint_build::compile("ui/app.slint").expect("ui/app.slint should compile");

    // Icon for the .exe itself (Explorer, taskbar, Start menu).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("ui/icon.ico")
            .compile()
            .expect("Windows resources should compile");
    }
}
