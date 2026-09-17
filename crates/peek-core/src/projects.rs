use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectItem {
    pub name: String,
    pub path: PathBuf,
    pub category: String,
    pub icon: &'static str,
}

pub struct ProjectManager {
    config_dir: PathBuf,
}

impl Default for ProjectManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectManager {
    /// Search only the chosen library. Group folders are traversed; repositories stop recursion.
    pub fn discover_library(root: &Path) -> Vec<PathBuf> {
        let mut pending = vec![(root.to_path_buf(), 0usize)];
        let mut projects = Vec::new();
        while let Some((directory, depth)) = pending.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            let mut children: Vec<_> = entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    entry.file_type().is_ok_and(|ft| ft.is_dir())
                        && !name.starts_with('.')
                        && !matches!(
                            name.as_ref(),
                            "node_modules" | "target" | "dist" | "venv" | "__pycache__"
                        )
                })
                .map(|entry| entry.path())
                .collect();
            children.sort();
            for path in children {
                if projects.len() >= 5000 {
                    return projects;
                }
                projects.push(path.clone());
                let repository = [
                    ".git",
                    "Cargo.toml",
                    "package.json",
                    "pyproject.toml",
                    "go.mod",
                ]
                .iter()
                .any(|marker| path.join(marker).exists());
                if depth < 3 && !repository {
                    pending.push((path, depth + 1));
                }
            }
        }
        projects
    }

    pub fn new() -> Self {
        let config_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("CodePeek");
        let _ = fs::create_dir_all(&config_dir);
        Self { config_dir }
    }

    fn recent_file_path(&self) -> PathBuf {
        self.config_dir.join("recent_projects.txt")
    }

    /// Loads the list of recent project paths from disk.
    pub fn load_recent(&self) -> Vec<PathBuf> {
        let file = self.recent_file_path();
        if let Ok(content) = fs::read_to_string(&file) {
            content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Adds a project path to the top of recent projects.
    pub fn add_recent(&self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut recents = self.load_recent();
        recents.retain(|p| p != &canonical);
        recents.insert(0, canonical);
        recents.truncate(25);

        let content = recents
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");

        let _ = fs::write(self.recent_file_path(), content);
    }

    /// Discovers workspace projects: recent projects, sibling directories, and parent folders.
    pub fn discover_projects(&self, current_root: &Path) -> Vec<ProjectItem> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        let current_canon = current_root
            .canonicalize()
            .unwrap_or_else(|_| current_root.to_path_buf());

        // 1. Add Recent Projects
        for p in self.load_recent() {
            if seen.insert(p.clone()) {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let icon = Self::detect_project_icon(&p);
                results.push(ProjectItem {
                    name,
                    icon,
                    category: "Reciente".to_string(),
                    path: p,
                });
            }
        }

        // 2. Scan Sibling directories of current project
        if let Some(parent) = current_canon.parent() {
            Self::scan_directory_for_projects(parent, "Workspace", &mut seen, &mut results);

            // Also check grandparent (e.g. dev/ or PersonalProjects/)
            if let Some(grandparent) = parent.parent() {
                Self::scan_directory_for_projects(
                    grandparent,
                    "Directorio Dev",
                    &mut seen,
                    &mut results,
                );
            }
        }

        // 3. Scan user's home dev directories
        if let Some(home) = dirs::home_dir() {
            for folder in ["dev", "projects", "PersonalProjects", "source", "repos"] {
                let dev_path = home.join(folder);
                if dev_path.is_dir() {
                    Self::scan_directory_for_projects(&dev_path, folder, &mut seen, &mut results);
                }
            }
        }

        results
    }

    fn scan_directory_for_projects(
        dir: &Path,
        category: &str,
        seen: &mut HashSet<PathBuf>,
        out: &mut Vec<ProjectItem>,
    ) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy();
                    // Skip hidden or build folders
                    if name_str.starts_with('.')
                        || matches!(
                            name_str.as_ref(),
                            "node_modules" | "target" | "dist" | ".next"
                        )
                    {
                        continue;
                    }

                    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if seen.insert(canon.clone()) {
                        let icon = Self::detect_project_icon(&canon);
                        out.push(ProjectItem {
                            name: name_str.to_string(),
                            icon,
                            category: category.to_string(),
                            path: canon,
                        });
                    }
                }
            }
        }
    }

    fn detect_project_icon(path: &Path) -> &'static str {
        if path.join("Cargo.toml").exists() {
            "\u{ea91}" // struct/code
        } else if path.join("package.json").exists() {
            "\u{ea8b}" // package
        } else if path.join("pyproject.toml").exists()
            || path.join("requirements.txt").exists()
            || path.join("go.mod").exists()
        {
            "\u{eae9}" // code
        } else if path.join(".git").exists() {
            "\u{eb46}" // root folder
        } else {
            "\u{ea83}" // folder
        }
    }
}
