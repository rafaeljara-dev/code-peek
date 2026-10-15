use anyhow::Result;

#[cfg(windows)]
mod native {
    use anyhow::{bail, Context, Result};
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    const INTEGRATION: &str = include_str!("../../../scripts/windows-integration.ps1");
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

    pub(super) fn install_dir() -> Result<PathBuf> {
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
        // Paths are environment data, never interpolated into PowerShell source.
        let system_root = std::env::var_os("SystemRoot").context("No se pudo localizar Windows")?;
        let powershell =
            PathBuf::from(system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = Command::new(powershell);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", INTEGRATION])
            // PowerShell 7 exports module paths incompatible with Windows PowerShell.
            // Let the system host initialize its own standard module search path.
            .env_remove("PSModulePath")
            .env("CODEPEEK_INSTALL_MODE", mode)
            .env("CODEPEEK_INSTALL_SOURCE", std::env::current_exe()?)
            .env("CODEPEEK_INSTALL_ASSETS", payload.path())
            .creation_flags(0x0800_0000);
        let output = command
            .output()
            .context("No se pudo ejecutar la integración de Windows")?;
        if !output.status.success() {
            bail!(
                "Falló la integración de Windows: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        Ok(())
    }
}

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

#[cfg(windows)]
pub fn launch_installed() -> Result<()> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(native::install_dir()?.join("peek.exe"))
        .creation_flags(0x0800_0000)
        .spawn()?;
    Ok(())
}
