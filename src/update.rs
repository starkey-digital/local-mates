use velopack::{UpdateCheck, UpdateManager, sources::GithubSource};

const REPO: &str = "https://github.com/starkey-digital/local-mates";

/// Downloads a newer release in the background. Velopack applies it on the next launch, so a
/// running session is never interrupted. Does nothing when not installed (e.g. `cargo run`).
pub fn spawn_check() {
    std::thread::spawn(|| {
        if let Err(err) = check() {
            tracing::debug!("update check skipped: {err}");
        }
    });
}

fn check() -> Result<(), velopack::Error> {
    let manager = UpdateManager::new(GithubSource::new(REPO, None, false), None, None)?;
    if let UpdateCheck::UpdateAvailable(update) = manager.check_for_updates()? {
        manager.download_updates(&update, None)?;
        println!(
            "Update {} downloaded; it installs next time you start local mates.",
            update.TargetFullRelease.Version
        );
    }
    Ok(())
}
