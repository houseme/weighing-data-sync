#[cfg(all(target_os = "windows", feature = "windows"))]
use std::env;

#[cfg(all(target_os = "windows", feature = "windows"))]
use anyhow::Context;

const RUN_VALUE_NAME: &str = "WeighingDataSync";

#[cfg(all(target_os = "windows", feature = "windows"))]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(all(target_os = "windows", feature = "windows"))]
pub fn enable_autostart() -> anyhow::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(RUN_KEY)
        .context("failed to open Windows Run registry key")?;

    let exe_path = env::current_exe().context("failed to resolve current executable path")?;
    let command = format!("\"{}\" run", exe_path.display());
    key.set_value(RUN_VALUE_NAME, &command)
        .context("failed to write Windows autostart registry value")?;

    tracing::info!(
        stage = "windows.autostart.enable",
        value = RUN_VALUE_NAME,
        command,
        "用户登录自启动项已写入"
    );
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "windows"))]
pub fn disable_autostart() -> anyhow::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(RUN_KEY, winreg::enums::KEY_SET_VALUE)
        .context("failed to open Windows Run registry key")?;

    match key.delete_value(RUN_VALUE_NAME) {
        Ok(()) => tracing::info!(
            stage = "windows.autostart.disable",
            value = RUN_VALUE_NAME,
            "用户登录自启动项已移除"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => tracing::info!(
            stage = "windows.autostart.disable",
            value = RUN_VALUE_NAME,
            "用户登录自启动项不存在，无需移除"
        ),
        Err(error) => {
            return Err(error).context("failed to delete Windows autostart registry value");
        }
    }
    Ok(())
}

#[cfg(all(target_os = "windows", feature = "windows"))]
pub fn is_autostart_enabled() -> anyhow::Result<bool> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey(RUN_KEY)
        .context("failed to open Windows Run registry key")?;
    Ok(key.get_value::<String, _>(RUN_VALUE_NAME).is_ok())
}

#[cfg(not(all(target_os = "windows", feature = "windows")))]
pub fn enable_autostart() -> anyhow::Result<()> {
    anyhow::bail!(
        "{} autostart requires Windows target and the `windows` feature",
        RUN_VALUE_NAME
    )
}

#[cfg(not(all(target_os = "windows", feature = "windows")))]
pub fn disable_autostart() -> anyhow::Result<()> {
    anyhow::bail!(
        "{} autostart requires Windows target and the `windows` feature",
        RUN_VALUE_NAME
    )
}

#[cfg(not(all(target_os = "windows", feature = "windows")))]
pub fn is_autostart_enabled() -> anyhow::Result<bool> {
    Ok(false)
}
