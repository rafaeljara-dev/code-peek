use anyhow::Result;

#[cfg(windows)]
mod native {
    use anyhow::{Context, Result};
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// Keeps PowerShell and the relaunched viewer from flashing a console window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const RESOURCES: &[(&str, &[u8])] = &[
        ("icon.ico", include_bytes!("../../../assets/icon.ico")),
        ("icon.png", include_bytes!("../../../assets/icon.png")),
        ("LICENSE", include_bytes!("../../../LICENSE")),
        (
            "THIRD-PARTY-NOTICES.md",
            include_bytes!("../../../THIRD-PARTY-NOTICES.md"),
        ),
        (
            "DEVICON-LICENSE",
            include_bytes!("../../peek-ui/assets/DEVICON-LICENSE"),
        ),
        (
            "CODICONS-LICENSE",
            include_bytes!("../../peek-ui/assets/CODICONS-LICENSE"),
        ),
        (
            "SLINT-LICENSE",
            include_bytes!("../../../licenses/SLINT-LICENSE.md"),
        ),
    ];

    fn install_dir() -> Result<PathBuf> {
        Ok(dirs::data_local_dir()
            .context("No se pudo localizar la carpeta de datos del usuario")?
            .join("Programs")
            .join("CodePeek"))
    }

    pub(super) fn integration(mode: &str) -> Result<()> {
        let payload = tempfile::tempdir()?;
        for (name, data) in RESOURCES {
            fs::write(payload.path().join(name), data)?;
        }
        let settings = peek_core::UserSettings::load().unwrap_or_default();
        let options = peek_core::WindowsIntegrationOptions::from(&settings);
        let message = peek_core::windows_integration::run_integration(
            mode,
            &std::env::current_exe()?,
            payload.path(),
            &options,
            env!("CARGO_PKG_VERSION"),
        )?;
        println!("{message}");
        Ok(())
    }

    pub fn launch_installed() -> Result<()> {
        Command::new(install_dir()?.join("peek.exe"))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    }
}

#[cfg(windows)]
pub use native::launch_installed;

pub fn install() -> Result<()> {
    #[cfg(windows)]
    return native::integration("install");
    #[cfg(not(windows))]
    anyhow::bail!(
        "La instalación integrada solo está disponible en Windows; consulta CONTRIBUTING.md"
    );
}

pub fn uninstall() -> Result<()> {
    #[cfg(windows)]
    return native::integration("uninstall");
    #[cfg(not(windows))]
    anyhow::bail!("La desinstalación integrada solo está disponible en Windows");
}

pub fn register_context_menu() -> Result<()> {
    #[cfg(windows)]
    return native::integration("context");
    #[cfg(not(windows))]
    anyhow::bail!("El menú contextual integrado solo está disponible en Windows");
}
