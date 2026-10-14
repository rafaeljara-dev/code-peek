use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_ICO_BYTES: &[u8] = include_bytes!("../../../assets/icon.ico");
const ICON_PNG_BYTES: &[u8] = include_bytes!("../../../assets/icon.png");

#[cfg(target_os = "windows")]
fn install_dir() -> Result<PathBuf> {
    let local_app_data =
        dirs::data_local_dir().context("Could not determine %LOCALAPPDATA% directory")?;
    Ok(local_app_data.join("Programs").join("CodePeek"))
}

#[cfg(target_os = "windows")]
pub fn launch_installed() -> Result<()> {
    let executable = install_dir()?.join("peek.exe");
    Command::new(&executable)
        .spawn()
        .with_context(|| format!("Failed to launch {}", executable.display()))?;
    Ok(())
}

/// Installs CodePeek as a first-class Windows application:
/// 1. Copies `peek.exe`, `icon.ico`, and `icon.png` to `%LOCALAPPDATA%\Programs\CodePeek\`
/// 2. Creates a Windows Start Menu shortcut with the custom app icon.
/// 3. Creates a Desktop shortcut with the custom app icon.
/// 4. Adds the install directory to User's `PATH` environment variable.
/// 5. Registers CodePeek in Windows "Installed apps" (Settings / Control Panel).
/// 6. Registers Windows Explorer context menu ("⚡ Abrir con CodePeek").
#[cfg(target_os = "windows")]
pub fn install() -> Result<()> {
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;

    let dest_dir = install_dir()?;
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create install directory: {}", dest_dir.display()))?;

    let dest_exe = dest_dir.join("peek.exe");
    let dest_ico = dest_dir.join("icon.ico");
    let dest_png = dest_dir.join("icon.png");

    // Copy executable if not already running from destination
    if current_exe != dest_exe {
        fs::copy(&current_exe, &dest_exe)
            .with_context(|| format!("Failed to copy binary to {}", dest_exe.display()))?;
    }

    // Write embedded high-res icon assets
    let _ = fs::write(&dest_ico, ICON_ICO_BYTES);
    let _ = fs::write(&dest_png, ICON_PNG_BYTES);

    let exe_str = dest_exe.to_string_lossy();
    let dest_dir_str = dest_dir.to_string_lossy();
    let ico_str = dest_ico.to_string_lossy();

    // 1. Create Start Menu Shortcut
    if let Some(app_data) = dirs::data_dir() {
        let start_menu_programs = app_data
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        let shortcut_path = start_menu_programs.join("CodePeek.lnk");

        let ps_cmd = format!(
            r#"$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{},0'; $s.Description = 'CodePeek - Ultra-lightweight native code inspector'; $s.WorkingDirectory = '{}'; $s.Save()"#,
            shortcut_path.to_string_lossy(),
            exe_str,
            ico_str,
            dest_dir_str
        );

        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output();
    }

    // 2. Create Desktop Shortcut
    if let Some(desktop) = dirs::desktop_dir() {
        let desktop_shortcut = desktop.join("CodePeek.lnk");

        let ps_cmd = format!(
            r#"$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{},0'; $s.Description = 'CodePeek - Ultra-lightweight native code inspector'; $s.WorkingDirectory = '{}'; $s.Save()"#,
            desktop_shortcut.to_string_lossy(),
            exe_str,
            ico_str,
            dest_dir_str
        );

        let _ = Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output();
    }

    // 3. Add to User PATH
    let ps_path_cmd = format!(
        r#"$old = [Environment]::GetEnvironmentVariable('PATH', 'User'); if ($old -notlike '*{}*') {{ [Environment]::SetEnvironmentVariable('PATH', "$old;{}", 'User') }}"#,
        dest_dir_str, dest_dir_str
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_path_cmd])
        .output();

    // 4. Register in Windows "Installed Apps" (HKCU Uninstall)
    let uninstall_key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek";
    let uninstaller_cmd = format!(r#""{}" --uninstall"#, exe_str);

    let reg_entries = [
        ("DisplayName", "CodePeek"),
        ("DisplayVersion", "0.1.0"),
        ("Publisher", "CodePeek Contributors"),
        ("DisplayIcon", ico_str.as_ref()),
        ("InstallLocation", &dest_dir_str),
        ("UninstallString", &uninstaller_cmd),
    ];

    for (name, val) in reg_entries {
        let _ = Command::new("reg")
            .args(["add", uninstall_key, "/v", name, "/d", val, "/f"])
            .output();
    }

    // 5. Register Context Menu
    register_context_menu_with_path(&dest_exe, &dest_ico)?;

    println!("===============================================================");
    println!("🎉 CodePeek ha sido instalado con éxito en Windows 11:");
    println!("   📁 Ubicación:      {}", dest_exe.display());
    println!("   🎨 Icono:          {}", dest_ico.display());
    println!("   🖥️  Escritorio:     Acceso directo 'CodePeek.lnk' con icono");
    println!("   🔍 Menú Inicio:    Disponible buscando 'CodePeek' o 'peek'");
    println!("   💻 Terminal/PATH:  Comando 'peek' disponible en cualquier consola");
    println!("   ⚙️ Menú Contextual: '⚡ Abrir con CodePeek' en clic derecho");
    println!("   📋 Configuración:  Registrado en 'Aplicaciones Instaladas'");
    println!("===============================================================");

    Ok(())
}

/// Uninstalls CodePeek from Windows:
#[cfg(target_os = "windows")]
pub fn uninstall() -> Result<()> {
    let dest_dir = install_dir()?;

    // 1. Remove Start Menu Shortcut
    if let Some(app_data) = dirs::data_dir() {
        let shortcut = app_data
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("CodePeek.lnk");
        let _ = fs::remove_file(shortcut);
    }

    // 2. Remove Desktop Shortcut
    if let Some(desktop) = dirs::desktop_dir() {
        let shortcut = desktop.join("CodePeek.lnk");
        let _ = fs::remove_file(shortcut);
    }

    // 3. Remove from User PATH
    let dest_dir_str = dest_dir.to_string_lossy();
    let ps_path_cmd = format!(
        r#"$old = [Environment]::GetEnvironmentVariable('PATH', 'User'); $new = ($old -split ';' | Where-Object {{ $_ -ne '{}' }}) -join ';'; [Environment]::SetEnvironmentVariable('PATH', $new, 'User')"#,
        dest_dir_str
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_path_cmd])
        .output();

    // 4. Remove Windows Uninstall entry
    let _ = Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CodePeek",
            "/f",
        ])
        .output();

    // 5. Remove Context Menus
    let _ = Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Classes\Directory\shell\CodePeek",
            "/f",
        ])
        .output();
    let _ = Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Classes\Directory\Background\shell\CodePeek",
            "/f",
        ])
        .output();
    let _ = Command::new("reg")
        .args(["delete", r"HKCU\Software\Classes\*\shell\CodePeek", "/f"])
        .output();

    // 6. Remove directory
    let _ = fs::remove_dir_all(&dest_dir);

    println!("✅ CodePeek ha sido desinstalado correctamente de Windows.");
    Ok(())
}

/// Registers context menu with custom icon for a specific executable path.
#[cfg(target_os = "windows")]
pub fn register_context_menu_with_path(exe_path: &Path, icon_path: &Path) -> Result<()> {
    let exe_str = exe_path.to_string_lossy();
    let icon_str = icon_path.to_string_lossy();

    // 1. Directory right-click
    let dir_key = r"HKCU\Software\Classes\Directory\shell\CodePeek";
    let dir_cmd_key = format!(r"{}\command", dir_key);
    let cmd_value = format!(r#""{}" "%V""#, exe_str);

    Command::new("reg")
        .args(["add", dir_key, "/ve", "/d", "⚡ Abrir con CodePeek", "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", dir_key, "/v", "Icon", "/d", &icon_str, "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", &dir_cmd_key, "/ve", "/d", &cmd_value, "/f"])
        .output()?;

    // 2. Directory background right-click
    let bg_key = r"HKCU\Software\Classes\Directory\Background\shell\CodePeek";
    let bg_cmd_key = format!(r"{}\command", bg_key);
    Command::new("reg")
        .args(["add", bg_key, "/ve", "/d", "⚡ Abrir con CodePeek", "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", bg_key, "/v", "Icon", "/d", &icon_str, "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", &bg_cmd_key, "/ve", "/d", &cmd_value, "/f"])
        .output()?;

    // 3. Any file right-click
    let file_key = r"HKCU\Software\Classes\*\shell\CodePeek";
    let file_cmd_key = format!(r"{}\command", file_key);
    let file_cmd_value = format!(r#""{}" "%1""#, exe_str);
    Command::new("reg")
        .args(["add", file_key, "/ve", "/d", "⚡ Abrir con CodePeek", "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", file_key, "/v", "Icon", "/d", &icon_str, "/f"])
        .output()?;
    Command::new("reg")
        .args(["add", &file_cmd_key, "/ve", "/d", &file_cmd_value, "/f"])
        .output()?;

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn register_context_menu() -> Result<()> {
    let current_exe = std::env::current_exe().context("Failed to get current executable path")?;
    let dest_dir = install_dir()?;
    let icon_path = dest_dir.join("icon.ico");
    register_context_menu_with_path(&current_exe, &icon_path)?;
    println!("✅ Menú contextual de Windows 11 registrado con éxito.");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn install() -> Result<()> {
    println!("Installation is only supported on Windows.");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn uninstall() -> Result<()> {
    println!("Uninstall is only supported on Windows.");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn register_context_menu() -> Result<()> {
    println!("Context menu integration is only available on Windows.");
    Ok(())
}
