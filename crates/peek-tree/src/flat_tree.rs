use crate::icons::icon_for_path;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatNode {
    pub path: PathBuf,
    pub relative_path: String,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_expanded: bool,
    pub icon: &'static str,
}

#[derive(Debug, Clone)]
pub struct FlatTree {
    pub root: PathBuf,
    pub expanded_dirs: HashSet<PathBuf>,
}

impl FlatTree {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        let root = root
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| root.as_ref().to_path_buf());
        let mut expanded_dirs = HashSet::new();
        // Expand root by default
        expanded_dirs.insert(root.clone());

        Self {
            root,
            expanded_dirs,
        }
    }

    /// Toggles folder expansion state. Returns true if now expanded, false if collapsed.
    pub fn toggle_expand(&mut self, path: &Path) -> bool {
        if self.expanded_dirs.contains(path) {
            self.expanded_dirs.remove(path);
            false
        } else {
            self.expanded_dirs.insert(path.to_path_buf());
            true
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded_dirs.contains(path)
    }

    /// Generates the flattened visible list of nodes for UI rendering.
    pub fn visible_nodes(&self) -> Vec<FlatNode> {
        let mut nodes = Vec::new();
        self.collect_visible(&self.root, 0, &mut nodes);
        nodes
    }

    fn collect_visible(&self, dir: &Path, depth: usize, out: &mut Vec<FlatNode>) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        let mut dirs = Vec::new();
        let mut files = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    if crate::walker::is_excluded_dir(&name_str) {
                        continue;
                    }
                    dirs.push(path);
                } else if file_type.is_file() {
                    files.push(path);
                }
            }
        }

        dirs.sort();
        files.sort();

        // 1. Process directories first
        for dir_path in dirs {
            let name = dir_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let relative = dir_path
                .strip_prefix(&self.root)
                .unwrap_or(&dir_path)
                .to_string_lossy()
                .to_string();
            let is_expanded = self.expanded_dirs.contains(&dir_path);
            let icon = icon_for_path(&dir_path, true, is_expanded);

            out.push(FlatNode {
                path: dir_path.clone(),
                relative_path: relative,
                name,
                is_dir: true,
                depth,
                is_expanded,
                icon,
            });

            if is_expanded {
                self.collect_visible(&dir_path, depth + 1, out);
            }
        }

        // 2. Process files
        for file_path in files {
            let name = file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let relative = file_path
                .strip_prefix(&self.root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();
            let icon = icon_for_path(&file_path, false, false);

            out.push(FlatNode {
                path: file_path,
                relative_path: relative,
                name,
                is_dir: false,
                depth,
                is_expanded: false,
                icon,
            });
        }
    }
}
