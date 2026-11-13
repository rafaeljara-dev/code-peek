use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::UserSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsIntegrationOptions {
    pub context_menu: bool,
    pub open_with_markdown: bool,
    pub open_with_code: bool,
    pub open_with_docs: bool,
}

impl Default for WindowsIntegrationOptions {
    fn default() -> Self {
        Self {
            context_menu: true,
            open_with_markdown: true,
            open_with_code: true,
            open_with_docs: true,
        }
    }
}

impl From<&UserSettings> for WindowsIntegrationOptions {
    fn from(settings: &UserSettings) -> Self {
        Self {
            context_menu: settings.windows_context_menu,
            open_with_markdown: settings.windows_open_with_markdown,
            open_with_code: settings.windows_open_with_code,
            open_with_docs: settings.windows_open_with_docs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowsIntegrationStatus {
    pub supported: bool,
    pub installed: bool,
    pub executable_path: Option<PathBuf>,
    pub context_menu: bool,
    pub open_with_markdown: bool,
    pub open_with_code: bool,
    pub open_with_docs: bool,
}

pub fn installed_executable_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("Programs").join("CodePeek").join("peek.exe"))
}

pub fn option_env_pairs(options: &WindowsIntegrationOptions) -> [(&'static str, &'static str); 4] {
    let flag = |value: bool| if value { "1" } else { "0" };
    [
        ("CODEPEEK_ENABLE_CONTEXT_MENU", flag(options.context_menu)),
        (
            "CODEPEEK_OPEN_WITH_MARKDOWN",
            flag(options.open_with_markdown),
        ),
        ("CODEPEEK_OPEN_WITH_CODE", flag(options.open_with_code)),
        ("CODEPEEK_OPEN_WITH_DOCS", flag(options.open_with_docs)),
    ]
}

#[cfg(windows)]
mod native {
    use super::*;
    use anyhow::{bail, Context};
    use std::ffi::OsStr;
    use std::fs;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const INTEGRATION: &str = include_str!("../../../scripts/windows-integration.ps1");
    const ICON_ICO: &[u8] = include_bytes!("../../../assets/icon.ico");

    fn to_wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn key_exists(subkey: &str) -> bool {
        let wide = to_wide(subkey);
        let mut handle: HKEY = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide.as_ptr(), 0, KEY_READ, &mut handle) };
        if status == ERROR_SUCCESS {
            unsafe {
                RegCloseKey(handle);
            }
            true
        } else {
            false
        }
    }

    fn value_exists(subkey: &str, value_name: &str) -> bool {
        let wide_key = to_wide(subkey);
        let wide_val = to_wide(value_name);
        let mut handle: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide_key.as_ptr(),
                0,
                KEY_READ,
                &mut handle,
            )
        };
        if status != ERROR_SUCCESS {
            return false;
        }
        let query = unsafe {
            RegQueryValueExW(
                handle,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        unsafe {
            RegCloseKey(handle);
        }
        query == ERROR_SUCCESS
    }

    pub fn query_status() -> WindowsIntegrationStatus {
        let installed_path = installed_executable_path().filter(|path| path.is_file());
        WindowsIntegrationStatus {
            supported: true,
            installed: installed_path.is_some(),
            executable_path: installed_path,
            context_menu: key_exists(r"Software\Classes\*\shell\CodePeek")
                && key_exists(r"Software\Classes\Directory\shell\CodePeek"),
            open_with_markdown: value_exists(
                r"Software\Classes\.md\OpenWithProgids",
                "CodePeek.Document",
            ),
            open_with_code: value_exists(
                r"Software\Classes\.rs\OpenWithProgids",
                "CodePeek.Document",
            ),
            open_with_docs: value_exists(
                r"Software\Classes\.pdf\OpenWithProgids",
                "CodePeek.Document",
            ),
        }
    }

    pub fn apply_integration(options: &WindowsIntegrationOptions) -> Result<String> {
        let payload = tempfile::tempdir()?;
        fs::write(payload.path().join("icon.ico"), ICON_ICO)?;
        let source = installed_executable_path()
            .filter(|path| path.is_file())
            .unwrap_or(std::env::current_exe()?);
        run_integration(
            "configure",
            &source,
            payload.path(),
            options,
            env!("CARGO_PKG_VERSION"),
        )
    }

    pub fn run_integration(
        mode: &str,
        source: &Path,
        assets_dir: &Path,
        options: &WindowsIntegrationOptions,
        version: &str,
    ) -> Result<String> {
        let script_dir = tempfile::tempdir()?;
        let script_path = script_dir.path().join("windows-integration.ps1");
        // UTF-8 BOM guarantees Windows PowerShell 5.1 parses Spanish literals as UTF-8.
        fs::write(&script_path, format!("\u{FEFF}{INTEGRATION}"))?;

        let system_root = std::env::var_os("SystemRoot").context("No se pudo localizar Windows")?;
        let powershell =
            PathBuf::from(system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(powershell);
        command
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            // PowerShell 7 exports module paths incompatible with Windows PowerShell.
            .env_remove("PSModulePath")
            .env("CODEPEEK_INSTALL_MODE", mode)
            .env("CODEPEEK_INSTALL_SOURCE", source)
            .env("CODEPEEK_INSTALL_ASSETS", assets_dir)
            .env("CODEPEEK_INSTALL_VERSION", version)
            .creation_flags(CREATE_NO_WINDOW);
        for (key, value) in option_env_pairs(options) {
            command.env(key, value);
        }
        let output = command
            .output()
            .context("No se pudo ejecutar la integración de Windows")?;
        if !output.status.success() {
            bail!(
                "Falló la integración de Windows: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

pub fn query_status() -> WindowsIntegrationStatus {
    #[cfg(windows)]
    {
        native::query_status()
    }
    #[cfg(not(windows))]
    {
        WindowsIntegrationStatus::default()
    }
}

#[cfg(windows)]
pub fn run_integration(
    mode: &str,
    source: &Path,
    assets_dir: &Path,
    options: &WindowsIntegrationOptions,
    version: &str,
) -> Result<String> {
    native::run_integration(mode, source, assets_dir, options, version)
}

pub fn apply_integration(options: &WindowsIntegrationOptions) -> Result<String> {
    #[cfg(windows)]
    {
        native::apply_integration(options)
    }
    #[cfg(not(windows))]
    {
        let _ = options;
        anyhow::bail!("La integración de sistema solo está disponible en Windows");
    }
}

/// Opens an external http/https URL in the user's default browser without invoking a shell.
pub fn open_external_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("Solo se permiten enlaces http:// o https://");
    }
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let to_wide = |s: &str| -> Vec<u16> {
            OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let op = to_wide("open");
        let target = to_wide(url);
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (rc as isize) <= 32 {
            anyhow::bail!("No se pudo abrir el enlace externo");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(not(target_os = "macos"))]
        let cmd = "xdg-open";
        std::process::Command::new(cmd).arg(url).spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_env_pairs_reflect_selected_flags() {
        let options = WindowsIntegrationOptions {
            context_menu: false,
            open_with_markdown: true,
            open_with_code: false,
            open_with_docs: true,
        };
        assert_eq!(
            option_env_pairs(&options),
            [
                ("CODEPEEK_ENABLE_CONTEXT_MENU", "0"),
                ("CODEPEEK_OPEN_WITH_MARKDOWN", "1"),
                ("CODEPEEK_OPEN_WITH_CODE", "0"),
                ("CODEPEEK_OPEN_WITH_DOCS", "1"),
            ]
        );
    }

    #[test]
    fn options_derive_from_user_settings() {
        let settings = UserSettings {
            windows_context_menu: false,
            windows_open_with_markdown: true,
            windows_open_with_code: false,
            ..Default::default()
        };
        let options = WindowsIntegrationOptions::from(&settings);
        assert!(!options.context_menu);
        assert!(options.open_with_markdown);
        assert!(!options.open_with_code);
        assert!(options.open_with_docs);
    }
}
