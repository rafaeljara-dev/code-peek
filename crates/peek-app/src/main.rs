#![windows_subsystem = "windows"]

mod cli;
mod windows;

use clap::Parser;
use cli::Cli;

#[cfg(target_os = "windows")]
fn launched_as_setup() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .is_some_and(|name| name.contains("codepeek-setup"))
}

fn main() -> anyhow::Result<()> {
    // On Windows: If launched from a terminal (PowerShell/CMD), attach to parent console
    // for CLI stdout. If launched from GUI/shortcuts, no terminal window will ever appear!
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(dw_process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let args = Cli::parse();

    #[cfg(target_os = "windows")]
    if launched_as_setup()
        && args.path.is_none()
        && !args.uninstall
        && !args.install_context_menu
        && !args.uninstall_context_menu
    {
        windows::install()?;
        windows::launch_installed()?;
        return Ok(());
    }

    if args.install {
        windows::install()?;
        return Ok(());
    }

    if args.uninstall {
        windows::uninstall()?;
        return Ok(());
    }

    if args.install_context_menu {
        windows::register_context_menu()?;
        return Ok(());
    }

    if args.uninstall_context_menu {
        let msg = peek_core::windows_integration::apply_integration(
            &peek_core::WindowsIntegrationOptions {
                context_menu: false,
                open_with_markdown: false,
                open_with_code: false,
                open_with_docs: false,
            },
        )?;
        println!("{msg}");
        return Ok(());
    }

    // A file opened while CodePeek is running becomes a tab in that window.
    if let Some(path) = args.path.as_deref().filter(|_| !args.new_window) {
        if peek_core::single_instance::forward_to_running_instance(path) {
            return Ok(());
        }
    }

    peek_ui::run_app(args.path)?;
    Ok(())
}
