#[cfg(all(target_os = "windows", feature = "windows"))]
use std::ffi::OsString;

const SERVICE_NAME: &str = "WeighingDataSync";
#[cfg(all(target_os = "windows", feature = "windows"))]
const SERVICE_DISPLAY_NAME: &str = "Shangheng Weighing Data Sync";

#[cfg(all(target_os = "windows", feature = "windows"))]
pub fn install_service() -> anyhow::Result<()> {
    use anyhow::Context;
    use windows_service::{
        service::{ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
            .context("failed to open Windows service manager")?;
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: std::env::current_exe().context("failed to resolve executable path")?,
        launch_arguments: vec![OsString::from("run")],
        dependencies: Vec::new(),
        account_name: None,
        account_password: None,
    };

    let service = manager
        .create_service(&service_info, ServiceAccess::CHANGE_CONFIG)
        .context("failed to create Windows service")?;
    service
        .set_description("Reliable SQLite-backed weighing data synchronization daemon")
        .context("failed to set Windows service description")?;

    tracing::info!(
        stage = "windows.service.install",
        service = SERVICE_NAME,
        "Windows 服务已安装，启动类型为自动"
    );
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "windows"))]
pub fn uninstall_service() -> anyhow::Result<()> {
    use anyhow::Context;
    use windows_service::{
        service::ServiceAccess,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .context("failed to open Windows service manager")?;
    let service = manager
        .open_service(SERVICE_NAME, ServiceAccess::DELETE)
        .context("failed to open Windows service")?;
    service
        .delete()
        .context("failed to delete Windows service")?;

    tracing::info!(
        stage = "windows.service.uninstall",
        service = SERVICE_NAME,
        "Windows 服务已卸载"
    );
    Ok(())
}

#[cfg(not(all(target_os = "windows", feature = "windows")))]
pub fn install_service() -> anyhow::Result<()> {
    anyhow::bail!(
        "{} service installation requires Windows target and the `windows` feature",
        SERVICE_NAME
    )
}

#[cfg(not(all(target_os = "windows", feature = "windows")))]
pub fn uninstall_service() -> anyhow::Result<()> {
    anyhow::bail!(
        "{} service uninstallation requires Windows target and the `windows` feature",
        SERVICE_NAME
    )
}
