use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "peek",
    author,
    version,
    about = "⚡ Ultra-lightweight native code inspector in pure Rust"
)]
pub struct Cli {
    /// Ruta de archivo o carpeta; sin ruta usa el inicio configurado o el último proyecto
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Install CodePeek to %LOCALAPPDATA%\Programs\CodePeek, Start Menu, PATH, and Context Menu
    #[arg(long = "install")]
    pub install: bool,

    /// Uninstall CodePeek from Windows (removes shortcuts, PATH entry, and registry keys)
    #[arg(long = "uninstall")]
    pub uninstall: bool,

    /// Register "Abrir con CodePeek" in Windows context menu
    #[arg(long = "install-context-menu")]
    pub install_context_menu: bool,
}
