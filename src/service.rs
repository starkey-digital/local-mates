//! Windows service: installed once with admin, then the app runs unelevated and talks to it.

use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use windows_service::{
    define_windows_service,
    service::{
        Service, ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl,
        ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

use crate::{daemon, ipc};

const NAME: &str = "LocalMates";
const FILES: [&str; 2] = ["local-mates.exe", "wintun.dll"];

/// Runs as SYSTEM, so its binary must live somewhere only admins can write — never in the
/// per-user Velopack install directory.
fn install_dir() -> PathBuf {
    PathBuf::from(env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into()))
        .join("local mates")
}

pub fn install() -> Result<()> {
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .context("installing the service needs an admin terminal")?;
    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::CHANGE_CONFIG;

    let existing = manager.open_service(NAME, access).ok();
    if let Some(service) = &existing {
        stop(service)?;
    }

    let dir = install_dir();
    fs::create_dir_all(&dir)?;
    let src = env::current_exe()?;
    for file in FILES {
        fs::copy(src.with_file_name(file), dir.join(file))
            .with_context(|| format!("couldn't copy {file} into {}", dir.display()))?;
    }

    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "local mates".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: dir.join(FILES[0]),
        launch_arguments: vec!["daemon".into(), "--service".into()],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service = match existing {
        Some(service) => {
            service.change_config(&info)?;
            service
        }
        None => manager.create_service(&info, access)?,
    };
    service.set_description("Virtual LAN for playing LAN games with friends")?;
    service.start::<&str>(&[])?;

    println!("local mates service installed and running.");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("removing the service needs an admin terminal")?;
    let service = manager.open_service(
        NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    stop(&service)?;
    service.delete()?;
    drop(service);

    if let Err(err) = fs::remove_dir_all(install_dir()) {
        eprintln!("Couldn't remove {}: {err}", install_dir().display());
    }
    println!("local mates service removed.");
    Ok(())
}

fn stop(service: &Service) -> Result<()> {
    if service.query_status()?.current_state == ServiceState::Stopped {
        return Ok(());
    }
    service.stop()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while service.query_status()?.current_state != ServiceState::Stopped {
        anyhow::ensure!(Instant::now() < deadline, "the service didn't stop");
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

pub fn run() -> Result<()> {
    service_dispatcher::start(NAME, ffi_service_main)?;
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_args: Vec<OsString>) {
    if let Err(err) = run_service() {
        tracing::error!("service failed: {err:#}");
    }
}

fn run_service() -> Result<()> {
    let stop = CancellationToken::new();
    let on_stop = stop.clone();
    let status = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            on_stop.cancel();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    set_state(
        &status,
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    )?;

    let res = tokio::runtime::Runtime::new()?
        .block_on(stop.run_until_cancelled(async { daemon::run(ipc::listen()?).await }));

    set_state(
        &status,
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
    )?;
    res.unwrap_or(Ok(()))
}

fn set_state(
    status: &ServiceStatusHandle,
    state: ServiceState,
    accept: ServiceControlAccept,
) -> windows_service::Result<()> {
    status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })
}
