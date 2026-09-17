use std::path::Path;

/// Codicon font glyph constants (VS Code icon font, 100% vector, zero emojis)
pub mod codicons {
    pub const FOLDER: &str = "\u{ea83}";
    pub const FOLDER_OPENED: &str = "\u{eaf7}";
    pub const FILE: &str = "\u{ea7b}";
    pub const FILE_CODE: &str = "\u{eae9}";
    pub const FILE_MEDIA: &str = "\u{eaea}";
    pub const FILE_ZIP: &str = "\u{eaef}";
    pub const FILE_CONFIG: &str = "\u{ea8b}";
    pub const RUST: &str = "\u{ea91}";
    pub const ZAP: &str = "\u{ea86}";
    pub const SEARCH: &str = "\u{ea6d}";
    pub const ROOT_FOLDER: &str = "\u{eb46}";
    pub const GEAR: &str = "\u{eaf8}";
    pub const SIDEBAR: &str = "\u{ebf3}";
    pub const SAVE: &str = "\u{eb4b}";
    pub const CHEVRON_RIGHT: &str = "\u{eab6}";
    pub const CHEVRON_DOWN: &str = "\u{eab4}";
}

/// Returns a clean, modern codicon character for a given file or folder path.
pub fn icon_for_path(path: &Path, is_dir: bool, is_expanded: bool) -> &'static str {
    if !is_dir {
        if let Some((glyph, _)) = language_icon(path) {
            return glyph;
        }
    }
    if is_dir {
        if is_expanded {
            codicons::FOLDER_OPENED
        } else {
            codicons::FOLDER
        }
    } else {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match file_name.as_str() {
            "cargo.toml" | "cargo.lock" => codicons::RUST,
            "package.json" | "pnpm-lock.yaml" | "package-lock.json" => codicons::FILE_CONFIG,
            "dockerfile" | "docker-compose.yml" => codicons::FILE_CONFIG,
            ".gitignore" | ".gitmodules" => codicons::FILE_CONFIG,
            _ => match ext.as_str() {
                "rs" => codicons::RUST,
                "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "c" | "cpp" | "h" | "hpp"
                | "html" | "css" | "sh" | "ps1" => codicons::FILE_CODE,
                "json" | "toml" | "yaml" | "yml" | "ini" | "lock" | "env" => codicons::FILE_CONFIG,
                "svg" | "png" | "jpg" | "jpeg" | "webp" | "ico" => codicons::FILE_MEDIA,
                "zip" | "tar" | "gz" | "7z" => codicons::FILE_ZIP,
                _ => codicons::FILE,
            },
        }
    }
}

pub fn icon_family(path: &Path, is_dir: bool) -> &'static str {
    if !is_dir && language_icon(path).is_some() {
        "devicon"
    } else {
        "codicon"
    }
}

pub fn icon_color(path: &Path, is_dir: bool) -> (u8, u8, u8) {
    if is_dir {
        (225, 182, 111)
    } else {
        language_icon(path)
            .map(|(_, color)| color)
            .unwrap_or((157, 183, 207))
    }
}

type LanguageIcon = (&'static str, (u8, u8, u8));
fn language_icon(path: &Path) -> Option<LanguageIcon> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "dockerfile" || name.starts_with("docker-compose") {
        return Some(("\u{e9c3}", (90, 177, 243)));
    }
    if name.starts_with(".git") {
        return Some(("\u{ea2d}", (244, 126, 101)));
    }
    if name == "cargo.toml" || name == "cargo.lock" {
        return Some(("\u{ebe6}", (239, 164, 125)));
    }
    Some(match ext.as_str() {
        "ts" | "mts" | "cts" => ("\u{ec63}", (92, 172, 242)),
        "tsx" | "jsx" => ("\u{ebbc}", (100, 215, 242)),
        "js" | "mjs" | "cjs" => ("\u{ea81}", (241, 215, 80)),
        "md" | "mdx" | "markdown" => ("\u{eadb}", (133, 195, 243)),
        "rs" => ("\u{ebe6}", (239, 164, 125)),
        "py" => ("\u{eb9c}", (238, 202, 103)),
        "html" | "htm" => ("\u{ea67}", (242, 143, 104)),
        "css" | "scss" | "sass" => ("\u{e9a1}", (141, 161, 244)),
        "json" | "jsonc" => ("\u{ea94}", (235, 208, 109)),
        "vue" => ("\u{ec96}", (105, 204, 157)),
        "svelte" => ("\u{ec30}", (247, 149, 107)),
        "go" => ("\u{ea3d}", (114, 207, 226)),
        "java" => ("\u{ea7f}", (229, 151, 109)),
        "kt" | "kts" => ("\u{eab5}", (193, 148, 246)),
        "cs" => ("\u{e9a0}", (184, 147, 235)),
        "cpp" | "cc" | "cxx" | "hpp" => ("\u{e99a}", (118, 177, 233)),
        _ => return None,
    })
}
