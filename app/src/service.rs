//! One-time setup: installing the background service needs admin, so it's the one UAC prompt.

pub const CAN_SETUP: bool = cfg!(windows);

/// Runs `local-mates service install` (shipped next to the app) elevated, and waits for it.
#[cfg(windows)]
pub fn install() -> anyhow::Result<()> {
    use std::{os::windows::process::CommandExt, process::Command};

    use anyhow::{Context, bail};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let cli = std::env::current_exe()?.with_file_name("local-mates.exe");
    // The path goes through an environment variable so no quoting can break the command.
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$p = Start-Process -FilePath $env:LOCAL_MATES_CLI -ArgumentList 'service','install' \
             -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
        ])
        .env("LOCAL_MATES_CLI", &cli)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("couldn't start setup")?;
    if !status.success() {
        bail!("Setup didn't finish. If Windows asked for permission, choose Yes and try again.");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn install() -> anyhow::Result<()> {
    anyhow::bail!("start the service with: sudo local-mates daemon")
}
