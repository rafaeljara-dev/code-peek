use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use peek_core::{TextBuffer, UserSettings, Workspace};
use peek_syntax::ExtensionRegistry;
use peek_tree::{walk_project_files, FlatNode, FlatTree, FuzzySearcher};
use slint::{ModelRc, SharedString, VecModel, Weak};

use crate::{CodeLineItem, CodeSpanItem, FileItem, MainWindow, ProjectItem, SearchItem};

pub enum Command {
    OpenProject(PathBuf),
    OpenFile(PathBuf),
    CloseFile(PathBuf),
    CloseProject(PathBuf),
    Toggle(PathBuf, bool),
    Library(PathBuf),
    LibraryParent,
    SetDefault,
    GoDefault,
    Search(String, i32),
    Save,
    Revert,
    Reload,
    Edit(usize),
    Select(usize),
    Apply(String),
    Resolve(i32),
    Scroll(i32),
    ScrollTo(f32),
    Viewport(usize),
    Zoom(i32),
    ZoomUi(i32),
    ToggleTheme,
    Panels(bool, bool),
    WrapColumns(usize),
    Undo,
    Close,
    Find(String, bool),
    CancelEdit,
    IndexReady(u64, Vec<PathBuf>),
}

#[derive(Clone)]
struct ProjectSession {
    tree: FlatTree,
    file: Option<PathBuf>,
    top: usize,
    recent_files: Vec<PathBuf>,
}

/// Owns disk access and buffers on one worker. The event loop only receives viewport models.
pub struct Controller {
    window: Weak<MainWindow>,
    workspace: Workspace,
    original: Option<TextBuffer>,
    undo: Vec<TextBuffer>,
    settings: UserSettings,
    settings_writable: bool,
    tree: FlatTree,
    library: FlatTree,
    tree_nodes: Vec<FlatNode>,
    library_nodes: Vec<FlatNode>,
    project_catalog: Vec<PathBuf>,
    paths: Vec<PathBuf>,
    recent_files: Vec<PathBuf>,
    sessions: HashMap<PathBuf, ProjectSession>,
    syntax: ExtensionRegistry,
    top: usize,
    selected: usize,
    visible: usize,
    pending: Option<Command>,
    status: String,
    editing: bool,
    stamp: Option<(SystemTime, u64)>,
    navigation_dirty: bool,
    content_width: f32,
    last_find: String,
    wrap_columns: usize,
    top_fragment: usize,
    indexer: Option<Sender<(u64, PathBuf)>>,
    index_generation: Arc<AtomicU64>,
    indexing: bool,
    active_search: Option<(String, i32)>,
}

impl Controller {
    pub fn run(
        initial: Option<PathBuf>,
        window: Weak<MainWindow>,
        rx: Receiver<Command>,
        tx: Sender<Command>,
    ) {
        let generation = Arc::new(AtomicU64::new(0));
        let (indexer, index_rx) = mpsc::channel::<(u64, PathBuf)>();
        let index_generation = generation.clone();
        std::thread::spawn(move || {
            while let Ok(request) = index_rx.recv() {
                let (id, root) = index_rx.try_iter().last().unwrap_or(request);
                if let Some(paths) =
                    peek_tree::walker::walk_project_files_cancellable(&root, || {
                        index_generation.load(Ordering::Relaxed) != id
                    })
                {
                    if tx.send(Command::IndexReady(id, paths)).is_err() {
                        break;
                    }
                }
            }
        });
        let loaded = UserSettings::load();
        let settings_writable = loaded.is_ok();
        let settings = match loaded {
            Ok(s) => s,
            Err(e) => {
                let message = e.to_string();
                let _ = window.upgrade_in_event_loop(move |w| w.set_error_message(message.into()));
                UserSettings::default()
            }
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let needs_setup = initial.is_none()
            && !settings
                .default_project
                .as_ref()
                .is_some_and(|p| p.is_dir())
            && !settings.last_project.as_ref().is_some_and(|p| p.is_dir());
        let path = settings.startup_path(initial, cwd.clone());
        let root = if path.is_file() {
            path.parent().unwrap_or(&cwd).to_path_buf()
        } else {
            path.clone()
        };
        let library = settings
            .library_root
            .as_ref()
            .filter(|p| p.is_dir())
            .cloned()
            .unwrap_or_else(|| root.parent().unwrap_or(&root).to_path_buf());
        let mut app = Self {
            window,
            workspace: Workspace::default(),
            original: None,
            undo: Vec::new(),
            settings,
            settings_writable,
            tree: FlatTree::new(&root),
            library: FlatTree::new(library),
            tree_nodes: Vec::new(),
            library_nodes: Vec::new(),
            project_catalog: Vec::new(),
            paths: Vec::new(),
            recent_files: Vec::new(),
            sessions: HashMap::new(),
            syntax: ExtensionRegistry::new(),
            top: 0,
            selected: 0,
            visible: 40,
            pending: None,
            status: String::new(),
            editing: false,
            stamp: None,
            navigation_dirty: true,
            content_width: 0.0,
            last_find: String::new(),
            wrap_columns: 100,
            top_fragment: 0,
            indexer: Some(indexer),
            index_generation: generation,
            indexing: false,
            active_search: None,
        };
        if needs_setup {
            app.status = "Abre un proyecto; quedará guardado en tu lista".into();
            {
                let _ = app.window.upgrade_in_event_loop(move |w| {
                    w.set_path_input(cwd.to_string_lossy().as_ref().into());
                    w.set_show_path(true);
                });
            }
        } else {
            let result = app.open_project(&root).and_then(|()| {
                if path.is_file() {
                    app.open_file(&path)
                } else {
                    Ok(())
                }
            });
            if let Err(e) = result {
                app.error(e);
            }
        }
        app.publish();
        loop {
            let command = match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(command) => command,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(e) = app.check_external() {
                        app.error(e);
                    }
                    continue;
                }
            };
            if matches!(command, Command::Close) && !app.workspace.is_dirty() {
                app.index_generation.fetch_add(1, Ordering::Relaxed);
                let _ = slint::quit_event_loop();
                break;
            }
            let closing = matches!(command, Command::Resolve(1 | 2))
                && matches!(app.pending, Some(Command::Close));
            app.set_busy(true);
            if let Err(e) = app.handle(command) {
                app.error(e);
            }
            app.publish();
            if closing && app.pending.is_none() {
                break;
            }
        }
    }

    fn error(&self, error: anyhow::Error) {
        let message = format!("{error:#}");
        let _ = self
            .window
            .upgrade_in_event_loop(move |w| w.set_error_message(message.into()));
    }

    fn set_busy(&self, busy: bool) {
        let _ = self.window.upgrade_in_event_loop(move |w| w.set_busy(busy));
    }

    fn persist(&self) {
        if self.settings_writable {
            if let Err(e) = self.settings.save() {
                self.error(e.context("No se pudo guardar la configuración"));
            }
        }
    }

    fn handle(&mut self, command: Command) -> Result<()> {
        if self.pending.is_some()
            && !matches!(
                command,
                Command::Resolve(_) | Command::Viewport(_) | Command::IndexReady(..)
            )
        {
            return Ok(());
        }
        // Guard the current buffer before anything can replace or discard it.
        let destructive = match &command {
            Command::CloseFile(p) => self.workspace.current_file.as_ref() == Some(p),
            Command::CloseProject(p) => self.workspace.root_dir.as_ref() == Some(p),
            Command::OpenFile(p) => self.workspace.current_file.as_ref() != Some(p),
            Command::OpenProject(p) => self.workspace.root_dir.as_ref() != Some(p),
            Command::Reload | Command::GoDefault | Command::Close => true,
            _ => false,
        };
        if destructive && self.editing {
            bail!("Aplica la edición con Enter o cancélala con Escape antes de continuar");
        }
        if destructive && self.workspace.is_dirty() {
            self.pending = Some(command);
            return Ok(());
        }
        if matches!(
            command,
            Command::OpenProject(_)
                | Command::OpenFile(_)
                | Command::Toggle(..)
                | Command::Library(_)
                | Command::LibraryParent
                | Command::SetDefault
                | Command::GoDefault
                | Command::Reload
        ) {
            self.navigation_dirty = true;
        }
        match command {
            Command::OpenProject(path) => self.open_project(&path)?,
            Command::CloseFile(path) => {
                if self.workspace.current_file.as_ref() == Some(&path) {
                    if let Some(next) = self.recent_files.iter().find(|p| **p != path).cloned() {
                        self.open_file(&next)?;
                    } else {
                        self.clear_file();
                    }
                }
                self.recent_files.retain(|p| p != &path);
                self.navigation_dirty = true;
            }
            Command::CloseProject(path) => {
                if self.workspace.root_dir.as_ref() == Some(&path) {
                    if let Some(next) = self
                        .settings
                        .open_projects
                        .iter()
                        .find(|p| **p != path && p.is_dir())
                        .cloned()
                    {
                        self.open_project(&next)?;
                    } else {
                        self.clear_file();
                        self.workspace.root_dir = None;
                        self.settings.last_project = None;
                        self.tree_nodes.clear();
                        self.paths.clear();
                        self.index_generation.fetch_add(1, Ordering::Relaxed);
                        self.indexing = false;
                    }
                }
                self.settings.open_projects.retain(|p| p != &path);
                self.sessions.remove(&path);
                self.navigation_dirty = true;
                self.persist();
            }
            Command::OpenFile(path) => {
                if self.workspace.current_file.as_ref() != Some(&path) {
                    self.open_file(&path)?;
                }
            }
            Command::Toggle(path, library) => {
                let tree = if library { &self.library } else { &self.tree };
                if !tree.is_expanded(&path) {
                    directory(&path)?;
                }
                if library {
                    self.library.toggle_expand(&path);
                    self.refresh_library();
                } else {
                    self.tree.toggle_expand(&path);
                    self.tree_nodes = self.tree.visible_nodes();
                }
            }
            Command::Library(path) => {
                let path = directory(&path)?;
                self.library = FlatTree::new(&path);
                self.refresh_library();
                self.project_catalog = peek_core::ProjectManager::discover_library(&path);
                self.settings.library_root = Some(path);
                self.persist();
            }
            Command::LibraryParent => {
                if let Some(parent) = self.library.root.parent().map(Path::to_path_buf) {
                    self.handle(Command::Library(parent))?;
                }
            }
            Command::SetDefault => {
                if self.settings.default_project == self.workspace.root_dir {
                    self.settings.default_project = None;
                    self.status = "Inicio desactivado: se abrirá el último proyecto".into();
                } else {
                    self.settings.default_project = self.workspace.root_dir.clone();
                    self.status = "Esta carpeta se abrirá al iniciar CodePeek".into();
                }
                self.persist();
            }
            Command::GoDefault => {
                if let Some(path) = self.settings.default_project.clone() {
                    self.open_project(&path)?;
                }
            }
            Command::Search(query, mode) => {
                self.active_search = Some((query.clone(), mode));
                self.search(&query, mode);
            }
            Command::Save => self.save()?,
            Command::Revert => {
                if let Some(original) = self.original.clone() {
                    self.workspace.buffer = Some(original);
                    self.undo.clear();
                    self.editing = false;
                    self.status = "Cambios revertidos".into();
                    let _ = self.window.upgrade_in_event_loop(|w| {
                        w.set_show_edit(false);
                        w.set_edit_text("".into());
                        w.invoke_focus_content();
                    });
                }
            }
            Command::Reload => {
                directory(&self.tree.root)?;
                self.tree_nodes = self.tree.visible_nodes();
                self.start_index();
                self.refresh_library();
                if let Some(path) = self.workspace.current_file.clone() {
                    self.open_file(&path)?;
                }
                self.status = "Archivos recargados desde disco".into();
            }
            Command::Edit(line) => {
                let buffer = self
                    .workspace
                    .buffer
                    .as_ref()
                    .context("Abre un archivo para editar")?;
                let text = buffer.line_trimmed(line).context("La línea ya no existe")?;
                if text.len() > 32_768 {
                    bail!("Esta línea supera el límite de edición de 32 KB");
                }
                self.selected = line;
                self.top_fragment = 0;
                self.editing = true;
                let _ = self.window.upgrade_in_event_loop(move |w| {
                    w.set_edit_text(text.into());
                    w.set_show_edit(true);
                });
            }
            Command::Select(line) => self.selected = line,
            Command::Apply(text) => {
                let buffer = self
                    .workspace
                    .buffer
                    .as_mut()
                    .context("No hay archivo abierto")?;
                if text.len() > 32_768 {
                    bail!("Esta línea supera el límite de edición de 32 KB");
                }
                let previous = buffer.clone();
                buffer.replace_line(self.selected, &text)?;
                self.editing = false;
                if buffer.revision != previous.revision {
                    self.undo.push(previous);
                    if self.undo.len() > 50 {
                        self.undo.remove(0);
                    }
                }
                let _ = self.window.upgrade_in_event_loop(|w| {
                    w.set_show_edit(false);
                    w.set_edit_text("".into());
                    w.invoke_focus_content();
                });
                self.status = "Cambio aplicado · Ctrl+S para guardar · Ctrl+Z para deshacer".into();
            }
            Command::Resolve(choice) => {
                if choice == 0 {
                    self.pending = None;
                } else {
                    if choice == 2 {
                        self.save()?;
                    }
                    if let Some(command) = self.pending.take() {
                        // Keep a recovery snapshot until navigation succeeds.
                        let recovery = self.workspace.buffer.clone();
                        if let Some(buffer) = self.workspace.buffer.as_mut() {
                            buffer.is_dirty = false;
                        }
                        if let Err(e) = self.handle(command) {
                            self.workspace.buffer = recovery;
                            return Err(e);
                        }
                    }
                }
                let _ = self
                    .window
                    .upgrade_in_event_loop(|w| w.invoke_focus_content());
            }
            Command::Scroll(lines) => {
                if let Some(buffer) = &self.workspace.buffer {
                    crate::wrapping::advance(
                        buffer,
                        &mut self.top,
                        &mut self.top_fragment,
                        lines,
                        self.wrap_columns,
                    );
                }
                self.keep_selection_visible();
            }
            Command::ScrollTo(ratio) => {
                self.top = (ratio.clamp(0.0, 1.0) * self.max_scroll() as f32).round() as usize;
                self.top_fragment = 0;
                if ratio >= 1.0 {
                    if let Some(buffer) = &self.workspace.buffer {
                        self.top_fragment =
                            crate::wrapping::row_count(buffer, self.top, self.wrap_columns) - 1;
                        crate::wrapping::advance(
                            buffer,
                            &mut self.top,
                            &mut self.top_fragment,
                            -(self.visible as i32 - 1),
                            self.wrap_columns,
                        );
                    }
                }
                self.keep_selection_visible();
            }
            Command::ZoomUi(delta) => {
                self.settings.ui_font_size =
                    (self.settings.ui_font_size + delta as f32).clamp(12.0, 22.0);
                self.persist();
            }
            Command::ToggleTheme => {
                self.settings.light_mode = !self.settings.light_mode;
                self.navigation_dirty = true;
                self.persist();
            }
            Command::Panels(show_projects, show_explorer) => {
                self.settings.show_projects = show_projects;
                self.settings.show_explorer = show_explorer;
                self.persist();
            }
            Command::WrapColumns(columns) => {
                self.wrap_columns = columns.max(4);
                self.top_fragment = 0;
            }
            Command::Viewport(lines) => {
                self.visible = lines.clamp(1, 300);
                self.top = self.top.min(self.max_scroll());
            }
            Command::Zoom(delta) => {
                self.settings.code_font_size =
                    (self.settings.code_font_size + delta as f32).clamp(9.0, 36.0);
                self.persist();
            }
            Command::Undo => {
                if let Some(mut previous) = self.undo.pop() {
                    previous.is_dirty = self
                        .original
                        .as_ref()
                        .is_some_and(|b| b.rope != previous.rope);
                    self.workspace.buffer = Some(previous);
                    self.status = "Último cambio deshecho".into();
                }
            }
            Command::Close => {
                self.index_generation.fetch_add(1, Ordering::Relaxed);
                let _ = slint::quit_event_loop();
            }
            Command::IndexReady(id, paths) => {
                if id == self.index_generation.load(Ordering::Relaxed) {
                    self.paths = paths;
                    self.indexing = false;
                    if let Some((query, mode)) = &self.active_search {
                        self.search(query, *mode);
                    }
                }
            }
            Command::CancelEdit => self.editing = false,
            Command::Find(query, forward) => {
                let buffer = self
                    .workspace
                    .buffer
                    .as_ref()
                    .context("Abre un archivo para buscar texto")?;
                if query.is_empty() {
                    return Ok(());
                }
                let same_query = self.last_find == query;
                self.last_find = query.clone();
                if let Some(line) = find_line(buffer, &query, self.selected, forward, same_query) {
                    self.selected = line;
                    self.top = line.saturating_sub(self.visible / 3).min(self.max_scroll());
                    self.top_fragment = 0;
                    self.status = "Coincidencia encontrada · Enter para la siguiente".into();
                } else {
                    self.status = "No se encontró ese texto en el archivo".into();
                }
            }
        }
        Ok(())
    }

    fn open_project(&mut self, path: &Path) -> Result<()> {
        let root = directory(path)?;
        if self.workspace.root_dir.as_ref() == Some(&root) {
            return Ok(());
        }
        if let Some(previous) = &self.workspace.root_dir {
            self.sessions.insert(
                previous.clone(),
                ProjectSession {
                    tree: self.tree.clone(),
                    file: self.workspace.current_file.clone(),
                    top: self.top,
                    recent_files: self.recent_files.clone(),
                },
            );
        }
        let session = self.sessions.get(&root).cloned();
        self.tree = session
            .as_ref()
            .map(|s| s.tree.clone())
            .unwrap_or_else(|| FlatTree::new(&root));
        self.tree_nodes = self.tree.visible_nodes();
        self.workspace = Workspace::new(Some(root.clone()));
        self.original = None;
        self.undo.clear();
        self.top = 0;
        self.top_fragment = 0;
        self.selected = 0;
        self.stamp = None;
        self.content_width = 0.0;
        self.navigation_dirty = true;
        self.recent_files = session
            .as_ref()
            .map(|s| s.recent_files.clone())
            .unwrap_or_default();
        self.settings.visit(&root);
        self.persist();
        self.paths.clear();
        self.active_search = None;
        self.start_index();
        self.status = "Ctrl+P para abrir · .env visibles".into();
        if let Some(session) = session {
            if let Some(file) = session.file.filter(|p| p.is_file()) {
                if let Err(e) = self.open_file(&file) {
                    self.error(e);
                }
                self.top = session.top.min(self.max_scroll());
            }
        }
        self.reset_horizontal();
        Ok(())
    }

    fn clear_file(&mut self) {
        self.workspace.close_file();
        self.original = None;
        self.undo.clear();
        self.stamp = None;
        self.top = 0;
        self.top_fragment = 0;
        self.selected = 0;
        self.recent_files.clear();
    }

    fn open_file(&mut self, path: &Path) -> Result<()> {
        // Read first: a missing/binary/invalid file must not clear the current buffer.
        let buffer = TextBuffer::from_file(path)?;
        let canonical = buffer.path.clone().context("No se pudo resolver la ruta")?;
        self.workspace.current_file = Some(canonical.clone());
        self.original = Some(buffer.clone());
        self.workspace.buffer = Some(buffer);
        self.undo.clear();
        self.top = 0;
        self.top_fragment = 0;
        self.selected = 0;
        self.stamp = file_stamp(&canonical);
        self.content_width = 0.0;
        self.last_find.clear();
        self.navigation_dirty = true;
        self.recent_files.retain(|p| p != &canonical);
        self.recent_files.insert(0, canonical.clone());
        let mut parent = canonical.parent();
        while let Some(dir) = parent.filter(|p| p.starts_with(&self.tree.root)) {
            self.tree.expanded_dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
        self.tree_nodes = self.tree.visible_nodes();
        self.status = "Selecciona una línea · F2 editar · F5 recargar".into();
        self.reset_horizontal();
        Ok(())
    }

    fn reset_horizontal(&self) {
        let _ = self
            .window
            .upgrade_in_event_loop(|w| w.set_horizontal_offset(0.0));
    }

    fn start_index(&mut self) {
        let id = self.index_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(indexer) = &self.indexer {
            self.indexing = indexer.send((id, self.tree.root.clone())).is_ok();
        } else {
            self.paths = walk_project_files(&self.tree.root);
            self.indexing = false;
        }
    }

    fn save(&mut self) -> Result<()> {
        let buffer = self
            .workspace
            .buffer
            .as_mut()
            .context("No hay archivo abierto")?;
        if !buffer.is_dirty {
            return Ok(());
        }
        let original = self
            .original
            .as_ref()
            .context("No hay versión original del archivo")?;
        peek_core::atomic_save::save_if_unchanged(buffer, original)?;
        self.original = Some(buffer.clone());
        self.stamp = buffer.path.as_ref().and_then(|p| file_stamp(p));
        self.status = "Archivo guardado".into();
        Ok(())
    }

    fn refresh_library(&mut self) {
        self.library_nodes = self
            .library
            .visible_nodes()
            .into_iter()
            .filter(|n| n.is_dir)
            .collect();
    }

    fn search(&self, query: &str, mode: i32) {
        let mut matcher = FuzzySearcher::new();
        let results: Vec<(String, String)> = if mode == 2 {
            let mut paths = self.settings.recent_projects.clone();
            paths.extend(self.settings.open_projects.iter().cloned());
            let mut seen = std::collections::HashSet::new();
            paths.retain(|p| seen.insert(p.clone()) && p.is_dir());
            matcher
                .search(query, &paths, 60)
                .into_iter()
                .map(|m| {
                    let display = format!("{}  —  {}", name(&m.path), display_path(&m.path));
                    (m.path.to_string_lossy().into_owned(), display)
                })
                .collect()
        } else {
            let relative: Vec<PathBuf> = self
                .paths
                .iter()
                .filter(|p| mode != 3 || is_environment(p))
                .map(|p| p.strip_prefix(&self.tree.root).unwrap_or(p).to_path_buf())
                .collect();
            matcher
                .search(query, &relative, 60)
                .into_iter()
                .map(|m| {
                    (
                        self.tree.root.join(&m.path).to_string_lossy().into_owned(),
                        display_path(&m.path),
                    )
                })
                .collect()
        };
        let query = query.to_owned();
        let project = self.tree.root.to_string_lossy().into_owned();
        let _ = self.window.upgrade_in_event_loop(move |w| {
            if w.get_palette_mode() == mode
                && w.get_query().as_str() == query
                && w.get_project_path().as_str() == project
            {
                w.set_search_results(search_model(results));
            }
        });
    }

    fn max_scroll(&self) -> usize {
        self.workspace
            .buffer
            .as_ref()
            .map_or(0, |b| b.line_count().saturating_sub(1))
    }

    fn keep_selection_visible(&mut self) {
        if self.selected < self.top || self.selected >= self.top + self.visible {
            self.selected = self.top;
        }
    }

    fn check_external(&mut self) -> Result<()> {
        let Some(path) = self.workspace.current_file.clone() else {
            return Ok(());
        };
        let stamp = file_stamp(&path);
        if stamp == self.stamp {
            return Ok(());
        }
        if self.editing || self.pending.is_some() {
            return Ok(());
        }
        if self.workspace.is_dirty() {
            self.stamp = stamp;
            bail!("El archivo cambió en disco. Tus cambios locales se conservan. F5 permite recargar.");
        }
        let top = self.top;
        let selected = self.selected;
        // Avoid repeatedly reporting a deleted or unreadable file on every idle tick.
        self.stamp = stamp;
        self.open_file(&path)?;
        self.top = top.min(self.max_scroll());
        self.selected = selected.min(
            self.workspace
                .buffer
                .as_ref()
                .map_or(1, TextBuffer::line_count)
                .saturating_sub(1),
        );
        self.status = "Actualizado desde disco".into();
        self.publish();
        Ok(())
    }

    fn publish(&mut self) {
        let navigation_dirty = std::mem::take(&mut self.navigation_dirty);
        let project = self.workspace.root_dir.clone().unwrap_or_default();
        let project_name = name(&project);
        let project_path = project.to_string_lossy().into_owned();
        let library_path = display_path(&self.library.root);
        let default_path = self
            .settings
            .default_project
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file = self.workspace.current_file.clone().unwrap_or_default();
        let current_file = display_path(file.strip_prefix(&project).unwrap_or(&file));
        let selected_path = file.to_string_lossy().into_owned();
        let tree = if navigation_dirty {
            self.tree_nodes.clone()
        } else {
            Vec::new()
        };
        let library = if navigation_dirty {
            self.library_nodes.clone()
        } else {
            Vec::new()
        };
        let recent_projects = self
            .settings
            .open_projects
            .iter()
            .map(|p| (p.to_string_lossy().into_owned(), name(p)))
            .collect();
        let recent_files = self
            .recent_files
            .iter()
            .map(|p| {
                (
                    p.to_string_lossy().into_owned(),
                    display_path(p.strip_prefix(&project).unwrap_or(p)),
                )
            })
            .collect();
        let dirty = self.workspace.is_dirty();
        let confirm = self.pending.is_some();
        let indexing = self.indexing;
        let selected = self.selected as i32 + 1;
        let selected_text = self
            .workspace
            .buffer
            .as_ref()
            .and_then(|b| b.line_trimmed(self.selected))
            .unwrap_or_default();
        let font = self.settings.code_font_size;
        let ui_font = self.settings.ui_font_size;
        let light_mode = self.settings.light_mode;
        let show_projects = self.settings.show_projects;
        let show_explorer = self.settings.show_explorer;
        let total = self
            .workspace
            .buffer
            .as_ref()
            .map_or(0, TextBuffer::line_count);
        let ratio = if self.max_scroll() > 0 {
            self.top as f32 / self.max_scroll() as f32
        } else {
            0.0
        };
        let status = if let Some(b) = &self.workspace.buffer {
            format!(
                "Línea {} / {}   UTF-8{}   {}   {}",
                selected,
                total,
                if b.has_bom { " BOM" } else { "" },
                if b.line_ending == peek_core::LineEnding::Crlf {
                    "CRLF"
                } else {
                    "LF"
                },
                self.status
            )
        } else {
            self.status.clone()
        };
        let mut rows = Vec::new();
        if let Some(buffer) = &self.workspace.buffer {
            let mut line = self.top;
            while rows.len() < self.visible && line < total {
                let source = buffer.line_trimmed(line).unwrap_or_default();
                let highlighted = self.syntax.highlight_viewport(
                    buffer,
                    self.workspace.current_file.as_deref(),
                    line,
                    1,
                );
                let spans = highlighted
                    .first()
                    .map(|l| l.spans.as_slice())
                    .unwrap_or(&[]);
                let wrapped = crate::wrapping::wrap(&source, spans, self.wrap_columns);
                let skip = if line == self.top {
                    self.top_fragment.min(wrapped.len() - 1)
                } else {
                    0
                };
                for (fragment, spans) in wrapped.into_iter().enumerate().skip(skip) {
                    if rows.len() >= self.visible {
                        break;
                    }
                    rows.push(crate::wrapping::Row {
                        line: line + 1,
                        continuation: fragment > 0,
                        spans,
                    });
                }
                line += 1;
            }
        }
        let visible_ratio =
            rows.iter().filter(|row| !row.continuation).count().max(1) as f32 / total.max(1) as f32;
        let _ = self.window.upgrade_in_event_loop(move |w| {
            w.set_project_name(project_name.into());
            w.set_project_path(project_path.into());
            w.set_library_path(library_path.into());
            w.set_default_path(default_path.into());
            w.set_current_file_path(current_file.into());
            w.set_selected_path(selected_path.into());
            if navigation_dirty {
                w.set_tree_items(tree_model(tree, light_mode));
                w.set_library_items(tree_model(library, light_mode));
                w.set_recent_projects(project_model(recent_projects));
                w.set_recent_files(search_model(recent_files));
            }
            w.set_is_dirty(dirty);
            w.set_show_confirm(confirm);
            w.set_selected_line(selected);
            w.set_code_font_size(font);
            w.set_ui_font_size(ui_font);
            w.set_light_mode(light_mode);
            w.set_show_library(show_projects);
            w.set_show_tree(show_explorer);
            w.set_total_lines(total as i32);
            w.set_scroll_ratio(ratio);
            w.set_visible_ratio(visible_ratio);
            w.set_status_info(status.into());

            w.set_busy(false);
            w.set_indexing(indexing);
            w.set_selected_text(selected_text.into());
            let items = rows
                .into_iter()
                .map(|row| CodeLineItem {
                    line_number: row.line as i32,
                    continuation: row.continuation,
                    spans: ModelRc::new(VecModel::from(
                        row.spans
                            .into_iter()
                            .map(|span| CodeSpanItem {
                                text: span.text.into(),
                                color: crate::theme::foreground(
                                    span.color.r,
                                    span.color.g,
                                    span.color.b,
                                    light_mode,
                                ),
                            })
                            .collect::<Vec<_>>(),
                    )),
                })
                .collect::<Vec<_>>();
            w.set_code_lines(ModelRc::new(VecModel::from(items)));
        });
    }
}

fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn find_line(
    buffer: &TextBuffer,
    query: &str,
    selected: usize,
    forward: bool,
    skip_current: bool,
) -> Option<usize> {
    let total = buffer.line_count();
    let selected = selected.min(total.saturating_sub(1));
    let start = usize::from(skip_current);
    (start..total + start)
        .map(|offset| {
            if forward {
                (selected + offset) % total
            } else {
                (selected + total - offset % total) % total
            }
        })
        .find(|&index| {
            buffer
                .line_trimmed(index)
                .is_some_and(|line| line.contains(query))
        })
}

fn directory(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("La carpeta no existe o no se puede leer")?;
    if !path.is_dir() {
        bail!("La ruta debe ser una carpeta");
    }
    std::fs::read_dir(&path).context("No se puede leer esta carpeta")?;
    Ok(path)
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| display_path(path))
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{unc}")
    } else {
        text.strip_prefix("\\\\?\\").unwrap_or(&text).to_owned()
    }
}

fn is_environment(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == ".env" || s.starts_with(".env."))
}

fn tree_model(nodes: Vec<FlatNode>, light_mode: bool) -> ModelRc<FileItem> {
    ModelRc::new(VecModel::from(
        nodes
            .into_iter()
            .map(|n| FileItem {
                path: n.path.to_string_lossy().as_ref().into(),
                name: n.name.into(),
                is_dir: n.is_dir,
                depth: n.depth as i32,
                is_expanded: n.is_expanded,
                icon: n.icon.into(),
                icon_family: peek_tree::icons::icon_family(&n.path, n.is_dir).into(),
                icon_color: {
                    let (r, g, b) = peek_tree::icons::icon_color(&n.path, n.is_dir);
                    crate::theme::foreground(r, g, b, light_mode)
                },
            })
            .collect::<Vec<_>>(),
    ))
}

fn search_model(items: Vec<(String, String)>) -> ModelRc<SearchItem> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(|(path, display_path)| SearchItem {
                path: SharedString::from(path),
                display_path: SharedString::from(display_path),
            })
            .collect::<Vec<_>>(),
    ))
}

fn project_model(items: Vec<(String, String)>) -> ModelRc<ProjectItem> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(|(path, display_path)| {
                let logo = project_logo(Path::new(&path));
                ProjectItem {
                    path: path.into(),
                    display_path: display_path.into(),
                    has_logo: logo.is_some(),
                    logo: logo.unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>(),
    ))
}

fn project_logo(root: &Path) -> Option<slint::Image> {
    const CANDIDATES: &[&str] = &[
        "favicon.ico",
        "icon.png",
        "public/favicon.ico",
        "public/favicon.png",
        "public/icon.png",
        "app/favicon.ico",
        "app/icon.png",
        "src/app/favicon.ico",
        "src/app/icon.png",
        "src/favicon.ico",
        "static/favicon.ico",
        "assets/favicon.ico",
    ];
    CANDIDATES
        .iter()
        .map(|relative| root.join(relative))
        .find_map(|path| load_project_image(&path))
}

fn load_project_image(path: &Path) -> Option<slint::Image> {
    if !path.is_file() {
        return None;
    }
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ico"))
    {
        let file = std::fs::File::open(path).ok()?;
        let directory = ico::IconDir::read(file).ok()?;
        let entry = directory
            .entries()
            .iter()
            .max_by_key(|entry| entry.width())?;
        let icon = entry.decode().ok()?;
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            icon.rgba_data(),
            icon.width(),
            icon.height(),
        );
        Some(slint::Image::from_rgba8(buffer))
    } else {
        slint::Image::load_from_path(path).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::ComponentHandle;
    use std::fs;

    fn fixture(root: &Path) -> (MainWindow, Controller) {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        let mut app = Controller {
            window: window.as_weak(),
            workspace: Workspace::default(),
            original: None,
            undo: Vec::new(),
            settings: UserSettings::default(),
            settings_writable: false,
            tree: FlatTree::new(root),
            library: FlatTree::new(root),
            tree_nodes: Vec::new(),
            library_nodes: Vec::new(),
            paths: Vec::new(),
            project_catalog: Vec::new(),
            recent_files: Vec::new(),
            sessions: HashMap::new(),
            syntax: ExtensionRegistry::new(),
            top: 0,
            selected: 0,
            visible: 3,
            pending: None,
            status: String::new(),
            editing: false,
            stamp: None,
            navigation_dirty: true,
            content_width: 0.0,
            last_find: String::new(),
            wrap_columns: 100,
            top_fragment: 0,
            indexer: None,
            index_generation: Arc::new(AtomicU64::new(0)),
            indexing: false,
            active_search: None,
        };
        app.open_project(root).unwrap();
        (window, app)
    }

    #[test]
    fn dirty_navigation_cancel_save_discard_and_failed_load() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        fs::write(&first, "original\n").unwrap();
        fs::write(&second, "second\n").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&first).unwrap();
        app.handle(Command::Edit(0)).unwrap();
        app.handle(Command::Apply("edited".into())).unwrap();
        app.handle(Command::OpenFile(second.clone())).unwrap();
        assert!(app.pending.is_some());
        assert_eq!(
            app.workspace.current_file,
            Some(first.canonicalize().unwrap())
        );
        app.handle(Command::Resolve(0)).unwrap();
        assert!(app.workspace.is_dirty());
        app.handle(Command::OpenFile(second.clone())).unwrap();
        app.handle(Command::Resolve(2)).unwrap();
        assert_eq!(fs::read_to_string(&first).unwrap(), "edited\n");
        assert_eq!(
            app.workspace.current_file,
            Some(second.canonicalize().unwrap())
        );
        app.handle(Command::Edit(0)).unwrap();
        app.handle(Command::Apply("discard me".into())).unwrap();
        app.handle(Command::OpenFile(dir.path().join("missing")))
            .unwrap();
        assert!(app.handle(Command::Resolve(1)).is_err());
        assert!(app.workspace.is_dirty());
        assert_eq!(
            app.workspace
                .buffer
                .as_ref()
                .unwrap()
                .line_trimmed(0)
                .as_deref(),
            Some("discard me")
        );
        app.handle(Command::OpenFile(first.clone())).unwrap();
        app.handle(Command::Resolve(1)).unwrap();
        assert_eq!(fs::read_to_string(second).unwrap(), "second\n");
    }

    #[test]
    fn project_sessions_restore_file_scroll_and_default() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("group").join("one");
        let second = dir.path().join("two");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let file = first.join(".env.local");
        fs::write(&file, "a\nb\nc\nd\ne\nf\ng\n").unwrap();
        let (_window, mut app) = fixture(&first);
        app.open_file(&file).unwrap();
        app.top = 3;
        app.handle(Command::SetDefault).unwrap();
        assert_eq!(
            app.settings.default_project,
            Some(first.canonicalize().unwrap())
        );
        app.open_project(&second).unwrap();
        assert!(app.workspace.buffer.is_none());
        app.handle(Command::GoDefault).unwrap();
        assert_eq!(
            app.workspace.current_file,
            Some(file.canonicalize().unwrap())
        );
        assert_eq!(app.top, 3);
        app.handle(Command::SetDefault).unwrap();
        assert!(app.settings.default_project.is_none());
        app.handle(Command::Library(dir.path().into())).unwrap();
        let group = dir.path().join("group").canonicalize().unwrap();
        app.handle(Command::Toggle(group, true)).unwrap();
        assert!(app
            .library_nodes
            .iter()
            .any(|n| n.name == "one" && n.depth == 1));
        let current_paths = app.paths.clone();
        let stale = app
            .index_generation
            .load(Ordering::Relaxed)
            .saturating_sub(1);
        app.handle(Command::IndexReady(stale, vec![PathBuf::from("stale")]))
            .unwrap();
        assert_eq!(app.paths, current_paths);
    }

    #[test]
    fn reload_conflicts_and_undo_keep_local_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, "before\n").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&path).unwrap();
        fs::write(&path, "external update\n").unwrap();
        app.check_external().unwrap();
        assert_eq!(
            app.workspace
                .buffer
                .as_ref()
                .unwrap()
                .line_trimmed(0)
                .as_deref(),
            Some("external update")
        );
        app.handle(Command::Edit(0)).unwrap();
        app.handle(Command::Apply("local".into())).unwrap();
        fs::write(&path, "new external update\n").unwrap();
        app.handle(Command::OpenProject(dir.path().join("missing")))
            .unwrap();
        assert!(app.handle(Command::Resolve(2)).is_err());
        assert!(app.pending.is_some());
        assert!(app.workspace.is_dirty());
        app.handle(Command::Resolve(0)).unwrap();
        app.handle(Command::Undo).unwrap();
        assert!(!app.workspace.is_dirty());
        assert_eq!(
            app.workspace
                .buffer
                .as_ref()
                .unwrap()
                .line_trimmed(0)
                .as_deref(),
            Some("external update")
        );
    }

    #[test]
    fn project_favicon_and_revert_are_native_and_lossless() {
        let dir = tempfile::tempdir().unwrap();
        let favicon = dir.path().join("favicon.ico");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon.ico"),
            &favicon,
        )
        .unwrap();
        assert!(project_logo(dir.path()).is_some());

        let source = dir.path().join("source.txt");
        fs::write(&source, "original\n").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&source).unwrap();
        app.handle(Command::Edit(0)).unwrap();
        let source = source.canonicalize().unwrap();
        assert!(app.handle(Command::CloseFile(source.clone())).is_err());
        assert_eq!(app.workspace.current_file, Some(source.clone()));
        app.handle(Command::Apply("changed".into())).unwrap();
        assert!(app.workspace.is_dirty());
        app.handle(Command::Revert).unwrap();
        assert!(!app.workspace.is_dirty());
        assert_eq!(
            app.workspace
                .buffer
                .as_ref()
                .unwrap()
                .line_trimmed(0)
                .as_deref(),
            Some("original")
        );
        assert_eq!(fs::read_to_string(source).unwrap(), "original\n");
    }

    #[test]
    fn closing_tabs_and_projects_preserves_dirty_changes_until_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&first).unwrap();
        app.open_file(&second).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();
        app.handle(Command::CloseFile(first.clone())).unwrap();
        assert_eq!(app.workspace.current_file, Some(second.clone()));
        assert!(!app.recent_files.contains(&first));
        app.handle(Command::Edit(0)).unwrap();
        app.handle(Command::Apply("changed".into())).unwrap();
        app.handle(Command::ToggleTheme).unwrap();
        assert!(app.settings.light_mode);
        assert!(app.workspace.is_dirty());
        assert_eq!(app.workspace.current_file, Some(second.clone()));
        app.handle(Command::ToggleTheme).unwrap();
        assert!(!app.settings.light_mode);
        app.handle(Command::CloseFile(second.clone())).unwrap();
        assert!(app.pending.is_some());
        app.handle(Command::Resolve(0)).unwrap();
        assert!(app.workspace.is_dirty());
        app.handle(Command::CloseFile(second.clone())).unwrap();
        app.handle(Command::Resolve(2)).unwrap();
        assert_eq!(fs::read_to_string(second).unwrap(), "changed");
        assert!(app.workspace.buffer.is_none());
        app.handle(Command::CloseProject(dir.path().canonicalize().unwrap()))
            .unwrap();
        assert!(app.settings.open_projects.is_empty());
        assert!(app.workspace.root_dir.is_none());
    }

    #[test]
    fn find_wraps_in_both_directions_and_handles_unicode() {
        let buffer = TextBuffer::from_text("niño\nother\nniño\nlast");
        assert_eq!(find_line(&buffer, "niño", 0, true, false), Some(0));
        assert_eq!(find_line(&buffer, "niño", 0, true, true), Some(2));
        assert_eq!(find_line(&buffer, "niño", 2, true, true), Some(0));
        assert_eq!(find_line(&buffer, "niño", 0, false, true), Some(2));
        assert_eq!(find_line(&buffer, "missing", 0, true, true), None);
    }
}
