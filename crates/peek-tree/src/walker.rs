use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

pub fn is_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | ".next"
            | "dist"
            | ".turbo"
            | ".cache"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}

/// Recursively scans a root directory using `ignore` engine (ripgrep technology).
/// Respects `.gitignore`, `.ignore`, and omits heavy developer directories (`node_modules`, `target`, `.git`).
pub fn walk_project_files<P: AsRef<Path>>(root: P) -> Vec<PathBuf> {
    walk_project_files_cancellable(root.as_ref(), || false).unwrap_or_default()
}

/// Cancel between directory entries when the user switches to another project.
pub fn walk_project_files_cancellable(
    root: &Path,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            // Fast filter for notorious garbage folders
            !entry
                .file_type()
                .is_some_and(|ft| ft.is_dir() && is_excluded_dir(&name))
        });

    // Collect ignored env files in a shallow pass over each visited directory.

    for entry in builder.build().flatten() {
        if cancelled() {
            return None;
        }
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            if let Ok(entries) = std::fs::read_dir(entry.path()) {
                for child in entries.flatten() {
                    if cancelled() {
                        return None;
                    }
                    let name = child.file_name();
                    let name = name.to_string_lossy();
                    if (name == ".env" || name.starts_with(".env."))
                        && child.file_type().is_ok_and(|ft| ft.is_file())
                    {
                        files.push(child.path());
                    }
                }
            }
        } else if entry.file_type().is_some_and(|ft| ft.is_file()) {
            files.push(entry.into_path());
        }
    }

    files.sort();
    files.dedup();
    if cancelled() {
        None
    } else {
        Some(files)
    }
}
