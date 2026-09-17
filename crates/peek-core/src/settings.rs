use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub light_mode: bool,
    pub show_projects: bool,
    pub show_explorer: bool,
    pub code_font_size: f32,
    pub tree_font_size: f32,
    pub ui_font_size: f32,
    pub library_root: Option<PathBuf>,
    pub default_project: Option<PathBuf>,
    pub last_project: Option<PathBuf>,
    pub recent_projects: Vec<PathBuf>,
    pub open_projects: Vec<PathBuf>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            light_mode: false,
            show_projects: true,
            show_explorer: true,
            code_font_size: 14.0,
            tree_font_size: 13.0,
            ui_font_size: 15.0,
            library_root: None,
            default_project: None,
            last_project: None,
            recent_projects: Vec::new(),
            open_projects: Vec::new(),
        }
    }
}

impl UserSettings {
    pub fn config_file_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("CodePeek")
            .join("settings.json")
    }

    pub fn load() -> Result<Self> {
        Self::load_from(&Self::config_file_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let mut settings: Self = match fs::read_to_string(path) {
            Ok(content) => {
                let value: serde_json::Value = serde_json::from_str(&content)
                    .context("La configuración no es válida; no se sobrescribirá")?;
                let migrate = value.get("open_projects").is_none();
                let migrate_zoom = value.get("ui_font_size").is_none();
                let mut loaded: Self = serde_json::from_value(value)?;
                if migrate {
                    loaded.open_projects = loaded.recent_projects.clone();
                }
                if migrate_zoom {
                    loaded.ui_font_size = loaded.tree_font_size.max(15.0);
                }
                loaded
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut legacy = Self::default();
                if let Ok(text) = fs::read_to_string(path.with_file_name("settings.txt")) {
                    for line in text.lines() {
                        if let Some((key, value)) = line.split_once('=') {
                            if let Ok(size) = value.trim().parse() {
                                match key.trim() {
                                    "code_font_size" => legacy.code_font_size = size,
                                    "tree_font_size" => legacy.tree_font_size = size,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if let Ok(text) = fs::read_to_string(path.with_file_name("recent_projects.txt")) {
                    legacy.recent_projects = text
                        .lines()
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .take(25)
                        .collect();
                }
                legacy.open_projects = legacy.recent_projects.clone();
                legacy.ui_font_size = legacy.tree_font_size.max(15.0);
                legacy
            }
            Err(e) => return Err(e).context("No se pudo leer la configuración"),
        };
        settings.code_font_size = finite_size(settings.code_font_size, 14.0, 36.0);
        settings.tree_font_size = finite_size(settings.tree_font_size, 13.0, 28.0);
        settings.ui_font_size = finite_size(settings.ui_font_size, 15.0, 22.0).max(12.0);
        Ok(settings)
    }

    pub fn startup_path(&self, explicit: Option<PathBuf>, cwd: PathBuf) -> PathBuf {
        explicit
            .or_else(|| {
                self.default_project
                    .as_ref()
                    .filter(|p| p.is_dir())
                    .cloned()
            })
            .or_else(|| self.last_project.as_ref().filter(|p| p.is_dir()).cloned())
            .unwrap_or(cwd)
    }

    pub fn visit(&mut self, path: &Path) {
        self.last_project = Some(path.to_path_buf());
        if !self.open_projects.iter().any(|p| p == path) {
            self.open_projects.push(path.to_path_buf());
        }
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(25);
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_file_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::save_atomic(path, &serde_json::to_string_pretty(self)?)
    }
}

fn finite_size(value: f32, fallback: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(9.0, max)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_roundtrip_and_startup_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let default = dir.path().join("predeterminado=ñ");
        let last = dir.path().join("último");
        fs::create_dir(&default).unwrap();
        fs::create_dir(&last).unwrap();
        let mut s = UserSettings {
            default_project: Some(default.clone()),
            library_root: Some(dir.path().into()),
            ..Default::default()
        };
        s.visit(&last);
        s.visit(&last);
        let path = dir.path().join("settings.json");
        s.save_to(&path).unwrap();
        let s = UserSettings::load_from(&path).unwrap();
        assert_eq!(s.recent_projects.len(), 1);
        assert_eq!(s.startup_path(None, "fallback".into()), default);
        assert_eq!(
            s.startup_path(Some("explicit".into()), "fallback".into()),
            PathBuf::from("explicit")
        );
        fs::remove_dir(&default).unwrap();
        assert_eq!(s.startup_path(None, "fallback".into()), last);
    }
    #[test]
    fn invalid_configuration_is_not_silently_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "broken").unwrap();
        assert!(UserSettings::load_from(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken");
    }

    #[test]
    fn project_list_and_independent_zoom_survive_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"recent_projects":["group/one","other/two"],"code_font_size":20,"tree_font_size":17}"#).unwrap();
        let mut settings = UserSettings::load_from(&path).unwrap();
        assert_eq!(settings.open_projects.len(), 2);
        assert_eq!(settings.ui_font_size, 17.0);
        assert!(settings.show_projects);
        assert!(settings.show_explorer);
        settings.visit(Path::new("deep/nested/three"));
        settings.visit(Path::new("group/one"));
        settings.ui_font_size = 19.0;
        settings.light_mode = true;
        settings.show_projects = false;
        settings.save_to(&path).unwrap();
        let mut restored = UserSettings::load_from(&path).unwrap();
        assert_eq!(restored.open_projects.len(), 3);
        assert_eq!(restored.code_font_size, 20.0);
        assert_eq!(restored.ui_font_size, 19.0);
        assert!(restored.light_mode);
        assert!(!restored.show_projects);
        assert!(restored.show_explorer);
        restored.open_projects.clear();
        restored.save_to(&path).unwrap();
        assert!(UserSettings::load_from(&path)
            .unwrap()
            .open_projects
            .is_empty());
    }
}
