#![windows_subsystem = "windows"]

mod cli;
mod windows;

use clap::Parser;
use cli::Cli;

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

    peek_ui::run_app(args.path)?;
    Ok(())
}
