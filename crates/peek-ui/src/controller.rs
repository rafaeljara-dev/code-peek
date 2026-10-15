use std::collections::{HashMap, HashSet};
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
use rayon::prelude::*;
use slint::{Model, ModelRc, SharedString, VecModel, Weak};
use std::sync::atomic::AtomicUsize;

use crate::editor::{self, EditSnapshot, EditorState};
use crate::image_loader;
use crate::thumbnails::{self, Images};
use crate::viewport::HighlightCache;
use crate::{CodeLineItem, CodeSpanItem, FileItem, MainWindow, ProjectItem, SearchItem};

pub enum Command {
    OpenProject(PathBuf),
    OpenFile(PathBuf),
    OpenAt(PathBuf, usize),
    CloseFile(PathBuf),
    CloseProject(PathBuf),
    Toggle(PathBuf, bool),
    Library(PathBuf),
    LibraryParent,
    SetDefault,
    GoDefault,
    Search(String, i32),
    Save,
    DocumentEdited(PathBuf, String),
    Revert,
    Reload,
    Edit(usize),
    Select(usize),
    Apply(String),
    Resolve(i32),
    Scroll(i32),
    EditorScroll(i32),
    ScrollTo(f32),
    EditorScrollTo(f32),
    EditorPan(i32),
    Viewport(usize),
    Zoom(i32),
    ZoomUi(i32),
    ToggleTheme,
    Panels(bool, bool),
    WrapColumns(usize),
    Undo,
    Redo,
    EditorAction(PathBuf, String, String, bool, bool),
    EditorPoint(PathBuf, usize, usize, bool),
    EditorCopy(bool),
    GoLine(String),
    FindAdvanced(String, bool, bool, bool),
    Replace(String, String, bool, bool, bool),
    Close,
    Find(String, bool),
    CancelEdit,
    IndexReady(u64, Vec<PathBuf>),
    ContentReady(u64, String, Vec<ContentMatch>),
    ThumbnailsReady(u64, Images, Images, bool),
    Preview(bool),
    TreeViewport(usize, usize),
}

#[derive(Clone)]
struct ContentRequest {
    id: u64,
    root: PathBuf,
    paths: Vec<PathBuf>,
    query: String,
}

#[derive(Clone)]
pub(crate) struct ContentMatch {
    path: PathBuf,
    line: usize,
    preview: String,
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
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    editor: EditorState,
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
    content_searcher: Option<Sender<ContentRequest>>,
    content_generation: Arc<AtomicU64>,
    content_searching: bool,
    is_image: bool,
    current_image: Option<image_loader::RawRgbaImage>,
    image_info: String,
    image_dirty: bool,
    thumbnail_loader: Option<Arc<thumbnails::Loader>>,
    tree_viewport: (usize, usize),
    thumbnail_generation: u64,
    thumbnails: Images,
    project_logos: Images,
    highlight_cache: HighlightCache,
    preview: bool,
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
        let index_tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(request) = index_rx.recv() {
                let (id, root) = index_rx.try_iter().last().unwrap_or(request);
                if let Some(paths) =
                    peek_tree::walker::walk_project_files_cancellable(&root, || {
                        index_generation.load(Ordering::Relaxed) != id
                    })
                {
                    if index_tx.send(Command::IndexReady(id, paths)).is_err() {
                        break;
                    }
                }
            }
        });
        let (content_searcher, content_rx) = mpsc::channel::<ContentRequest>();
        let content_generation = Arc::new(AtomicU64::new(0));
        let search_generation = content_generation.clone();
        let content_tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(request) = content_rx.recv() {
                let request = content_rx.try_iter().last().unwrap_or(request);
                let query = request.query.to_lowercase();
                let found = AtomicUsize::new(0);
                let matches = request
                    .paths
                    .par_iter()
                    .flat_map(|path| {
                        if search_generation.load(Ordering::Relaxed) != request.id
                            || found.load(Ordering::Relaxed) >= 60
                        {
                            return Vec::new();
                        }
                        search_content_file(path, &query)
                            .into_iter()
                            .filter(|_| found.fetch_add(1, Ordering::Relaxed) < 60)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                if search_generation.load(Ordering::Relaxed) == request.id
                    && content_tx
                        .send(Command::ContentReady(
                            request.id,
                            request.root.to_string_lossy().into_owned(),
                            matches,
                        ))
                        .is_err()
                {
                    break;
                }
            }
        });
        let thumb_tx = tx.clone();
        let thumbnail_loader =
            thumbnails::Loader::new(move |generation, files, projects, complete| {
                let _ = thumb_tx.send(Command::ThumbnailsReady(
                    generation, files, projects, complete,
                ));
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
            redo: Vec::new(),
            editor: EditorState::default(),
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
            content_searcher: Some(content_searcher),
            content_generation,
            content_searching: false,
            is_image: false,
            current_image: None,
            image_info: String::new(),
            image_dirty: false,
            thumbnail_loader: Some(Arc::new(thumbnail_loader)),
            tree_viewport: (0, 64),
            thumbnail_generation: 0,
            thumbnails: Images::new(),
            project_logos: Images::new(),
            highlight_cache: HighlightCache::default(),
            preview: false,
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
            if matches!(command, Command::ThumbnailsReady(..)) {
                if let Err(e) = app.handle(command) {
                    app.error(e);
                }
                continue;
            }
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
                Command::Resolve(_)
                    | Command::Viewport(_)
                    | Command::IndexReady(..)
                    | Command::ContentReady(..)
                    | Command::ThumbnailsReady(..)
                    | Command::Preview(_)
                    | Command::TreeViewport(..)
            )
        {
            return Ok(());
        }
        // Guard the current buffer before anything can replace or discard it.
        let destructive = match &command {
            Command::CloseFile(p) => self.workspace.current_file.as_ref() == Some(p),
            Command::CloseProject(p) => self.workspace.root_dir.as_ref() == Some(p),
            Command::OpenFile(p) => self.workspace.current_file.as_ref() != Some(p),
            Command::OpenAt(p, _) => self.workspace.current_file.as_ref() != Some(p),
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
                | Command::OpenAt(..)
                | Command::Toggle(..)
                | Command::Library(_)
                | Command::LibraryParent
                | Command::SetDefault
                | Command::GoDefault
                | Command::Reload
        ) {
            self.navigation_dirty = true;
        }
        if matches!(
            command,
            Command::Undo
                | Command::Redo
                | Command::Revert
                | Command::DocumentEdited(..)
                | Command::Apply(_)
                | Command::Replace(..)
        ) {
            self.highlight_cache.invalidate();
        }
        match command {
            Command::TreeViewport(first, count) => {
                let viewport = (first, count.clamp(1, 256));
                if self.tree_viewport != viewport {
                    self.tree_viewport = viewport;
                    self.navigation_dirty = true;
                }
            }
            Command::Preview(preview) => {
                self.preview = preview;
            }
            Command::ThumbnailsReady(generation, mut files, projects, complete) => {
                if generation == self.thumbnail_generation {
                    if !complete {
                        for (path, image) in &self.thumbnails {
                            files.entry(path.clone()).or_insert_with(|| image.clone());
                        }
                    }
                    let file_changes = image_changes(&self.thumbnails, &files);
                    let logo_changes = image_changes(&self.project_logos, &projects);
                    let file_updates =
                        row_updates(self.tree_nodes.iter().map(|n| &n.path), &file_changes);
                    let logo_updates =
                        row_updates(self.settings.open_projects.iter(), &logo_changes);
                    self.thumbnails = files;
                    self.project_logos = projects;
                    if !file_updates.is_empty() || !logo_updates.is_empty() {
                        let _ = self.window.upgrade_in_event_loop(move |w| {
                            apply_image_updates(&w, file_updates, logo_updates)
                        });
                    }
                }
            }
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
            Command::OpenAt(path, line) => {
                if self.workspace.current_file.as_ref() != Some(&path) {
                    self.open_file(&path)?;
                }
                self.selected = line.min(self.max_scroll());
                self.top = self
                    .selected
                    .saturating_sub(self.visible / 3)
                    .min(self.max_scroll());
                self.top_fragment = 0;
                self.reveal_editor_line();
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
            Command::DocumentEdited(path, text) => {
                if self.workspace.current_file.as_ref() == Some(&path) {
                    if let Some(buffer) = self.workspace.buffer.as_mut() {
                        let previous = buffer.clone();
                        buffer.replace_text(&text);
                        buffer.is_dirty = self
                            .original
                            .as_ref()
                            .is_none_or(|original| original.rope != buffer.rope);
                        if buffer.revision != previous.revision {
                            self.undo.push(EditSnapshot::new(previous, &self.editor));
                            self.redo.clear();
                            if self.undo.len() > 50 {
                                self.undo.remove(0);
                            }
                        }
                    }
                }
            }
            Command::Revert => {
                if let Some(original) = self.original.clone() {
                    self.workspace.buffer = Some(original);
                    self.undo.clear();
                    self.redo.clear();
                    self.editor = EditorState::default();
                    self.editing = false;
                    self.status = "Cambios revertidos".into();
                    self.sync_document();
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
                    self.undo.push(EditSnapshot::new(previous, &self.editor));
                    self.redo.clear();
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
                self.sync_document();
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
            Command::EditorScroll(lines) => {
                let max = self
                    .workspace
                    .buffer
                    .as_ref()
                    .map_or(0, |b| b.line_count().saturating_sub(self.visible));
                if lines >= 0 {
                    self.top = self.top.saturating_add(lines as usize).min(max);
                } else {
                    self.top = self.top.saturating_sub(lines.unsigned_abs() as usize);
                }
                self.top_fragment = 0;
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
            Command::EditorScrollTo(ratio) => {
                let max = self
                    .workspace
                    .buffer
                    .as_ref()
                    .map_or(0, |b| b.line_count().saturating_sub(self.visible));
                self.top = (ratio.clamp(0.0, 1.0) * max as f32).round() as usize;
                self.top_fragment = 0;
            }
            Command::EditorPan(delta) => {
                self.editor.left_column = if delta > 0 {
                    (self.editor.left_column + delta as usize).min(1_000_000)
                } else {
                    self.editor
                        .left_column
                        .saturating_sub(delta.unsigned_abs() as usize)
                };
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
            Command::Undo | Command::Redo => {
                let redo = matches!(command, Command::Redo);
                let snapshot = if redo {
                    self.redo.pop()
                } else {
                    self.undo.pop()
                };
                if let Some(mut snapshot) = snapshot {
                    if let Some(current) = self.workspace.buffer.take() {
                        let current = EditSnapshot::new(current, &self.editor);
                        if redo {
                            self.undo.push(current);
                        } else {
                            self.redo.push(current);
                        }
                    }
                    snapshot.buffer.is_dirty = self
                        .original
                        .as_ref()
                        .is_none_or(|b| b.rope != snapshot.buffer.rope);
                    self.workspace.buffer = Some(snapshot.buffer);
                    self.editor = snapshot.editor;
                    self.ensure_caret_visible();
                    self.status = if redo {
                        "Cambio rehecho"
                    } else {
                        "Cambio deshecho"
                    }
                    .into();
                }
            }
            Command::EditorAction(path, action, text, shift, control) => {
                if self.workspace.current_file.as_ref() != Some(&path) {
                    return Ok(());
                }
                if let Some(buffer) = self.workspace.buffer.as_mut() {
                    let snapshot = EditSnapshot::new(buffer.clone(), &self.editor);
                    let prefix = editor::comment_prefix(self.workspace.current_file.as_deref());
                    if action == "comment" && prefix.is_empty() {
                        bail!("Este lenguaje no tiene un comentario de línea compatible");
                    }
                    self.editor.command(
                        buffer,
                        &action,
                        if action == "comment" { prefix } else { &text },
                        shift,
                        control,
                    );
                    if buffer.revision != snapshot.buffer.revision {
                        self.highlight_cache.invalidate();
                        buffer.is_dirty = self
                            .original
                            .as_ref()
                            .is_none_or(|original| original.rope != buffer.rope);
                        self.undo.push(snapshot);
                        if self.undo.len() > 100 {
                            self.undo.remove(0);
                        }
                        self.redo.clear();
                    }
                    self.ensure_caret_visible();
                }
            }
            Command::EditorPoint(path, line, column, extend) => {
                if self.workspace.current_file.as_ref() == Some(&path) {
                    if let Some(buffer) = &self.workspace.buffer {
                        self.editor.point(buffer, line, column, extend);
                        self.selected = self.editor.position(buffer).0;
                    }
                    self.ensure_caret_visible();
                }
            }
            Command::EditorCopy(cut) => {
                if let Some(buffer) = &self.workspace.buffer {
                    let text = self.editor.selected_text(buffer);
                    if !text.is_empty() {
                        let _ = self
                            .window
                            .upgrade_in_event_loop(move |w| w.invoke_write_clipboard(text.into()));
                        if cut {
                            if let Some(path) = self.workspace.current_file.clone() {
                                self.handle(Command::EditorAction(
                                    path,
                                    "cut".into(),
                                    String::new(),
                                    false,
                                    false,
                                ))?;
                            }
                        }
                    }
                }
            }
            Command::GoLine(input) => {
                let line: usize = input
                    .trim()
                    .parse()
                    .context("Escribe un número de línea válido")?;
                let buffer = self
                    .workspace
                    .buffer
                    .as_ref()
                    .context("Abre un archivo para ir a una línea")?;
                if line == 0 || line > buffer.line_count() {
                    bail!("La línea debe estar entre 1 y {}", buffer.line_count());
                }
                self.editor.point(buffer, line - 1, 0, false);
                self.ensure_caret_visible();
                let _ = self.window.upgrade_in_event_loop(|w| {
                    w.set_show_goto(false);
                    w.invoke_focus_content();
                });
            }
            Command::FindAdvanced(query, forward, case, whole) => {
                self.find_advanced(&query, forward, case, whole)?;
            }
            Command::Replace(query, replacement, all, case, whole) => {
                let buffer = self
                    .workspace
                    .buffer
                    .as_mut()
                    .context("Abre un archivo para reemplazar texto")?;
                let ranges = editor::matches(buffer, &query, case, whole);
                if ranges.is_empty() {
                    self.status = "Sin coincidencias".into();
                    return Ok(());
                }
                let snapshot = EditSnapshot::new(buffer.clone(), &self.editor);
                let selection = self.editor.selection();
                let ranges = if all {
                    ranges
                } else {
                    vec![ranges
                        .iter()
                        .find(|r| **r == selection)
                        .or_else(|| ranges.iter().find(|r| r.start >= self.editor.cursor))
                        .unwrap_or(&ranges[0])
                        .clone()]
                };
                let count = ranges.len();
                let replacement = replacement
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', buffer.line_ending.as_str());
                for range in ranges.iter().rev() {
                    buffer.delete(range.clone());
                    buffer.insert(range.start, &replacement);
                }
                self.editor.anchor = ranges[0].start;
                self.editor.cursor = ranges[0].start + replacement.chars().count();
                buffer.is_dirty = self
                    .original
                    .as_ref()
                    .is_none_or(|original| original.rope != buffer.rope);
                self.undo.push(snapshot);
                self.redo.clear();
                if self.undo.len() > 100 {
                    self.undo.remove(0);
                }
                self.ensure_caret_visible();
                self.status = format!("{count} reemplazos · Ctrl+Z para deshacer");
                let _ = self.window.upgrade_in_event_loop(move |w| {
                    w.set_find_summary(format!("{count} reemplazos").into())
                });
            }
            Command::Close => {
                self.index_generation.fetch_add(1, Ordering::Relaxed);
                let _ = slint::quit_event_loop();
            }
            Command::IndexReady(id, paths) => {
                if id == self.index_generation.load(Ordering::Relaxed) {
                    self.paths = paths;
                    self.indexing = false;
                    if let Some((query, mode)) = self.active_search.clone() {
                        self.search(&query, mode);
                    }
                }
            }
            Command::ContentReady(id, root, matches) => {
                if id == self.content_generation.load(Ordering::Relaxed)
                    && root == self.tree.root.to_string_lossy()
                {
                    self.content_searching = false;
                    let expected_query = self
                        .active_search
                        .as_ref()
                        .filter(|(_, mode)| *mode == 4)
                        .map(|(query, _)| query.clone())
                        .unwrap_or_default();
                    let project = self.tree.root.clone();
                    let results = matches
                        .into_iter()
                        .map(|item| SearchItem {
                            display_path: format!(
                                "{}:{}  —  {}",
                                display_path(
                                    item.path.strip_prefix(&project).unwrap_or(&item.path)
                                ),
                                item.line,
                                item.preview
                            )
                            .into(),
                            path: item.path.to_string_lossy().as_ref().into(),
                            line_number: item.line as i32,
                        })
                        .collect::<Vec<_>>();
                    let _ = self.window.upgrade_in_event_loop(move |w| {
                        w.set_content_searching(false);
                        if w.get_palette_mode() == 4 && w.get_query().as_str() == expected_query {
                            w.set_search_results(ModelRc::new(VecModel::from(results)));
                        }
                    });
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
                    self.reveal_editor_line();
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
        self.redo.clear();
        self.editor = EditorState::default();
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
        self.content_generation.fetch_add(1, Ordering::Relaxed);
        self.content_searching = false;
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
        self.highlight_cache.invalidate();
        self.workspace.close_file();
        self.original = None;
        self.undo.clear();
        self.redo.clear();
        self.editor = EditorState::default();
        self.is_image = false;
        self.current_image = None;
        self.image_dirty = true;
        self.image_info.clear();
        self.stamp = None;
        self.top = 0;
        self.top_fragment = 0;
        self.selected = 0;
        self.recent_files.clear();
        let _ = self.window.upgrade_in_event_loop(|w| {
            w.set_is_image(false);
            w.set_image_source(slint::Image::default());
            w.set_image_info("".into());
        });
    }

    fn open_file(&mut self, path: &Path) -> Result<()> {
        if image_loader::is_image_file(path) {
            let details = image_loader::load_image_full(path)?;
            self.highlight_cache.invalidate();
            self.image_dirty = true;
            self.preview = false;
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            self.workspace.current_file = Some(canonical.clone());
            self.workspace.buffer = None;
            self.original = None;
            self.undo.clear();
            self.redo.clear();
            self.editor = EditorState::default();
            self.top = 0;
            self.top_fragment = 0;
            self.selected = 0;
            let raw = details.raw;
            self.is_image = true;
            self.current_image = Some(raw);
            self.image_info = details.summary.clone();
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
            self.status = format!("Imagen: {}", details.summary);
            let info = details.summary;
            let _ = self.window.upgrade_in_event_loop(move |w| {
                w.set_is_image(true);
                w.set_image_info(info.into());
                w.set_document_text("".into());
                w.set_markdown_file(false);
                w.set_preview(false);
                w.set_editor_selection_start(-1);
                w.set_editor_selection_end(-1);
            });
            self.reset_horizontal();
            return Ok(());
        }

        // Read first: a missing/binary/invalid file must not clear the current buffer.
        let buffer = TextBuffer::from_file(path)?;
        self.highlight_cache.invalidate();
        self.is_image = false;
        self.current_image = None;
        self.image_dirty = true;
        self.image_info.clear();
        let canonical = buffer.path.clone().context("No se pudo resolver la ruta")?;
        self.workspace.current_file = Some(canonical.clone());
        self.original = Some(buffer.clone());
        self.workspace.buffer = Some(buffer);
        self.undo.clear();
        self.redo.clear();
        self.editor = EditorState::default();
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
        self.status = "Ctrl+S guardar · Ctrl+P buscar archivos".into();
        self.sync_document();
        self.reset_horizontal();
        Ok(())
    }

    fn reveal_editor_line(&mut self) {
        if let Some(buffer) = &self.workspace.buffer {
            let line = self.selected.min(buffer.line_count().saturating_sub(1));
            self.editor.anchor = buffer.line_to_char(line);
            self.editor.cursor = self.editor.anchor
                + buffer
                    .line_trimmed(line)
                    .unwrap_or_default()
                    .chars()
                    .count();
        }
    }

    fn ensure_caret_visible(&mut self) {
        if let Some(buffer) = &self.workspace.buffer {
            self.editor.clamp(buffer);
            let (line, column) = self.editor.position(buffer);
            self.selected = line;
            if line < self.top {
                self.top = line;
            }
            if line >= self.top + self.visible {
                self.top = line.saturating_sub(self.visible - 1);
            }
            if column < self.editor.left_column {
                self.editor.left_column = column;
            }
            if column >= self.editor.left_column + self.wrap_columns {
                self.editor.left_column = column.saturating_sub(self.wrap_columns - 1);
            }
            self.top_fragment = 0;
        }
    }

    fn find_advanced(&mut self, query: &str, forward: bool, case: bool, whole: bool) -> Result<()> {
        let buffer = self
            .workspace
            .buffer
            .as_ref()
            .context("Abre un archivo para buscar texto")?;
        let ranges = editor::matches(buffer, query, case, whole);
        let selection = self.editor.selection();
        let same = self.last_find == query;
        self.last_find = query.into();
        let chosen = if forward {
            ranges
                .iter()
                .position(|r| r.start >= self.editor.cursor && (!same || *r != selection))
                .or((!ranges.is_empty()).then_some(0))
        } else {
            ranges
                .iter()
                .rposition(|r| r.start < selection.start && (!same || *r != selection))
                .or_else(|| ranges.len().checked_sub(1))
        };
        let summary = if let Some(index) = chosen {
            self.editor.anchor = ranges[index].start;
            self.editor.cursor = ranges[index].end;
            self.ensure_caret_visible();
            format!("{} de {}", index + 1, ranges.len())
        } else {
            "Sin coincidencias".into()
        };
        self.status = summary.clone();
        let _ = self
            .window
            .upgrade_in_event_loop(move |w| w.set_find_summary(summary.into()));
        Ok(())
    }

    fn sync_document(&mut self) {
        let markdown = self
            .workspace
            .current_file
            .as_deref()
            .and_then(|path| self.syntax.find_extension(path))
            .is_some_and(|extension| extension.is_markdown());
        if !markdown {
            self.preview = false;
        }
        let _ = self.window.upgrade_in_event_loop(move |w| {
            w.set_is_image(false);
            w.set_document_text("".into());
            w.set_editor_selection_start(-1);
            w.set_editor_selection_end(-1);
            w.set_markdown_file(markdown);
            if !markdown {
                w.set_preview(false);
            }
        });
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

    fn search(&mut self, query: &str, mode: i32) {
        if mode == 4 {
            let id = self.content_generation.fetch_add(1, Ordering::Relaxed) + 1;
            if query.chars().count() < 2 {
                self.content_searching = false;
                let _ = self.window.upgrade_in_event_loop(|w| {
                    w.set_content_searching(false);
                    w.set_content_query_short(true);
                    w.set_search_results(search_model(Vec::new()));
                });
                return;
            }
            self.content_searching = self.content_searcher.as_ref().is_some_and(|worker| {
                worker
                    .send(ContentRequest {
                        id,
                        root: self.tree.root.clone(),
                        paths: self.paths.clone(),
                        query: query.to_owned(),
                    })
                    .is_ok()
            });
            let searching = self.content_searching;
            let _ = self.window.upgrade_in_event_loop(move |w| {
                w.set_content_query_short(false);
                w.set_content_searching(searching);
            });
            return;
        }
        self.content_generation.fetch_add(1, Ordering::Relaxed);
        self.content_searching = false;
        let _ = self
            .window
            .upgrade_in_event_loop(|w| w.set_content_query_short(false));
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
        let mut thumbnail_job = None;
        if navigation_dirty {
            let request = thumbnails::Request {
                files: self
                    .tree_nodes
                    .iter()
                    .skip(self.tree_viewport.0.saturating_sub(16))
                    .take(self.tree_viewport.1 + 32)
                    .filter(|n| !n.is_dir)
                    .map(|n| n.path.clone())
                    .collect(),
                projects: self
                    .settings
                    .open_projects
                    .iter()
                    .take(100)
                    .cloned()
                    .collect(),
            };
            self.thumbnail_generation += 1;
            let files: HashSet<&PathBuf> = request.files.iter().collect();
            let projects: HashSet<&PathBuf> = request.projects.iter().collect();
            self.thumbnails.retain(|path, _| files.contains(path));
            self.project_logos.retain(|path, _| projects.contains(path));
            if let Some(loader) = &self.thumbnail_loader {
                thumbnail_job = Some((loader.clone(), self.thumbnail_generation, request));
            }
        }
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
            .map(|p| (p.to_string_lossy().into_owned(), name(p)))
            .collect();
        let dirty = self.workspace.is_dirty();
        let confirm = self.pending.is_some();
        let indexing = self.indexing;
        let content_searching = self.content_searching;
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
        let status = if self.is_image {
            self.status.clone()
        } else if let Some(b) = &self.workspace.buffer {
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
        let caret = self
            .workspace
            .buffer
            .as_ref()
            .map(|b| self.editor.position(b))
            .unwrap_or_default();
        let left = self.editor.left_column;
        let first_line = self.top as i32 + 1;
        let editor_max_scroll = total.saturating_sub(self.visible);
        let editor_ratio = if editor_max_scroll == 0 {
            0.0
        } else {
            self.top as f32 / editor_max_scroll as f32
        };
        let editor_visible = (self.visible as f32 / total.max(1) as f32).min(1.0);
        let can_undo = !self.undo.is_empty();
        let can_redo = !self.redo.is_empty();
        let language = self
            .workspace
            .current_file
            .as_deref()
            .map(|p| self.syntax.language_name(p))
            .unwrap_or_default();
        let mut editor_rows = Vec::new();
        if let Some(buffer) = self.workspace.buffer.as_ref().filter(|_| !self.preview) {
            let selection = self.editor.selection();
            let highlighted = self.highlight_cache.get(
                &self.syntax,
                buffer,
                self.workspace.current_file.as_deref(),
                self.top,
                self.visible,
                false,
            );
            for line in highlighted.iter() {
                let index = line.line_number - 1;
                let source = buffer.line_trimmed(index).unwrap_or_default();
                let start = buffer.line_to_char(index);
                let end = start + source.chars().count();
                let selection_start = selection.start.clamp(start, end) - start;
                let selection_end = selection.end.clamp(start, end) - start;
                let from = editor::display_width(
                    &source.chars().take(selection_start).collect::<String>(),
                );
                let mut to =
                    editor::display_width(&source.chars().take(selection_end).collect::<String>());
                if selection.end > end && selection.start <= end {
                    to += 1;
                }
                let spans = editor::display_spans(&source, &line.spans, left, self.wrap_columns);
                editor_rows.push((
                    line.line_number,
                    from.saturating_sub(left),
                    to.saturating_sub(left).min(self.wrap_columns),
                    spans,
                ));
            }
        }
        let mut rows = Vec::new();
        if let Some(buffer) = self.workspace.buffer.as_ref().filter(|_| self.preview) {
            let highlighted = self.highlight_cache.get(
                &self.syntax,
                buffer,
                self.workspace.current_file.as_deref(),
                self.top,
                self.visible,
                true,
            );
            let mut line = self.top;
            while rows.len() < self.visible && line < total {
                let source = buffer.line_trimmed(line).unwrap_or_default();
                let highlighted_line = highlighted.get(line - self.top);
                let spans = highlighted_line.map(|l| l.spans.as_slice()).unwrap_or(&[]);
                let line_type = highlighted_line.map_or(0, |l| l.line_type as i32);
                let is_markdown = highlighted_line.is_some_and(|l| l.is_markdown);
                let (skip, wrapped) = crate::wrapping::wrap_visible(
                    &source,
                    spans,
                    self.wrap_columns,
                    is_markdown,
                    if line == self.top {
                        self.top_fragment
                    } else {
                        0
                    },
                    self.visible - rows.len(),
                );
                for (fragment, spans) in wrapped.into_iter().enumerate() {
                    if rows.len() >= self.visible {
                        break;
                    }
                    rows.push(crate::wrapping::Row {
                        line: line + 1,
                        continuation: fragment + skip > 0,
                        line_type,
                        is_markdown,
                        spans,
                    });
                }
                line += 1;
            }
        }
        let visible_ratio =
            rows.iter().filter(|row| !row.continuation).count().max(1) as f32 / total.max(1) as f32;
        let is_image = self.is_image;
        let image_source =
            std::mem::take(&mut self.image_dirty).then(|| self.current_image.clone());
        let image_info = self.image_info.clone();
        // Images only feed rebuilt navigation models; other publishes skip the map copies.
        let (thumbs, logos) = if navigation_dirty {
            (self.thumbnails.clone(), self.project_logos.clone())
        } else {
            Default::default()
        };
        let _ = self.window.upgrade_in_event_loop(move |w| {
            w.set_caret_line(caret.0 as i32 + 1);
            w.set_caret_column(caret.1.saturating_sub(left) as i32);
            w.set_cursor_column(caret.1 as i32 + 1);
            w.set_first_line(first_line);
            w.set_editor_scroll_ratio(editor_ratio.min(1.0));
            w.set_editor_visible_ratio(editor_visible);
            w.set_left_column(left as i32);
            w.set_can_undo(can_undo);
            w.set_can_redo(can_redo);
            w.set_language_name(language.into());
            w.set_editor_rows(ModelRc::new(VecModel::from(
                editor_rows
                    .into_iter()
                    .map(|(line, from, to, spans)| crate::EditorRowItem {
                        line_number: line as i32,
                        selection_start: from as i32,
                        selection_end: to as i32,
                        spans: ModelRc::new(VecModel::from(
                            spans
                                .into_iter()
                                .map(|span| crate::EditorSpanItem {
                                    text: span.text.into(),
                                    column: span.column as i32,
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
                    .collect::<Vec<_>>(),
            )));
            w.set_project_name(project_name.into());
            w.set_project_path(project_path.into());
            w.set_library_path(library_path.into());
            w.set_default_path(default_path.into());
            w.set_current_file_path(current_file.into());
            w.set_selected_path(selected_path.into());
            w.set_is_image(is_image);
            if let Some(source) = image_source {
                w.set_image_source(image_loader::RawRgbaImage::to_slint_or_default(
                    source.as_ref(),
                ));
            }
            if is_image {
                w.set_image_info(image_info.into());
            }
            if navigation_dirty {
                w.set_tree_items(tree_model(tree, light_mode, &thumbs));
                w.set_library_items(tree_model(library, light_mode, &thumbs));
                w.set_recent_projects(project_model(recent_projects, &logos));
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
            w.set_content_searching(content_searching);
            w.set_selected_text(selected_text.into());
            let items = rows
                .into_iter()
                .map(|row| CodeLineItem {
                    line_number: row.line as i32,
                    continuation: row.continuation,
                    line_type: row.line_type,
                    is_markdown: row.is_markdown,
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
                                bold: span.bold,
                                italic: span.italic,
                            })
                            .collect::<Vec<_>>(),
                    )),
                })
                .collect::<Vec<_>>();
            w.set_code_lines(ModelRc::new(VecModel::from(items)));
            // Let the native window and its initial models publish before background I/O.
            if let Some((loader, generation, request)) = thumbnail_job {
                loader.request(generation, request);
            }
        });
    }
}

type ImageUpdate = (usize, String, Option<image_loader::RawRgbaImage>);

fn image_changes(
    old: &Images,
    new: &Images,
) -> HashMap<PathBuf, Option<image_loader::RawRgbaImage>> {
    let mut changes = HashMap::new();
    for (path, previous) in old {
        let image = new.get(path);
        if image
            .is_none_or(|image| image.data.as_bytes().as_ptr() != previous.data.as_bytes().as_ptr())
        {
            changes.insert(path.clone(), image.cloned());
        }
    }
    for (path, image) in new {
        if !old.contains_key(path) {
            changes.insert(path.clone(), Some(image.clone()));
        }
    }
    changes
}

/// Pair each changed path with the row index and path text it occupies in a navigation model.
fn row_updates<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
    changes: &HashMap<PathBuf, Option<image_loader::RawRgbaImage>>,
) -> Vec<ImageUpdate> {
    paths
        .enumerate()
        .filter_map(|(index, path)| {
            changes
                .get(path)
                .map(|image| (index, path.to_string_lossy().into_owned(), image.clone()))
        })
        .collect()
}

/// Update only existing matching rows; preserve the model, scroll and document properties.
fn apply_image_updates(window: &MainWindow, files: Vec<ImageUpdate>, logos: Vec<ImageUpdate>) {
    patch_rows(
        window.get_tree_items(),
        files,
        |row| &row.path,
        |row, image| {
            row.has_thumb = image.is_some();
            row.thumb = image_loader::RawRgbaImage::to_slint_or_default(image.as_ref());
        },
    );
    patch_rows(
        window.get_recent_projects(),
        logos,
        |row| &row.path,
        |row, image| {
            row.has_logo = image.is_some();
            row.logo = image_loader::RawRgbaImage::to_slint_or_default(image.as_ref());
        },
    );
}

fn patch_rows<T: Clone + 'static>(
    model: ModelRc<T>,
    updates: Vec<ImageUpdate>,
    path: impl Fn(&T) -> &SharedString,
    set: impl Fn(&mut T, Option<image_loader::RawRgbaImage>),
) {
    for (index, expected, image) in updates {
        if let Some(mut row) = model
            .row_data(index)
            .filter(|row| path(row).as_str() == expected)
        {
            set(&mut row, image);
            model.set_row_data(index, row);
        }
    }
}

fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn search_content_file(path: &Path, query: &str) -> Vec<ContentMatch> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Vec::new();
    };
    if metadata.len() > 4 * 1024 * 1024 {
        return Vec::new();
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(query))
        .take(3)
        .map(|(index, line)| ContentMatch {
            path: path.to_path_buf(),
            line: index + 1,
            preview: line.trim().chars().take(180).collect(),
        })
        .collect()
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

fn tree_model(nodes: Vec<FlatNode>, light_mode: bool, thumbs: &Images) -> ModelRc<FileItem> {
    ModelRc::new(VecModel::from(
        nodes
            .into_iter()
            .map(|n| {
                let thumb = thumbs.get(&n.path).map(|r| r.to_slint_image());
                let has_thumb = thumb.is_some();
                FileItem {
                    path: n.path.to_string_lossy().as_ref().into(),
                    name: n.name.into(),
                    is_dir: n.is_dir,
                    depth: n.depth as i32,
                    is_expanded: n.is_expanded,
                    icon: n.icon.into(),
                    icon_family: peek_tree::icons::icon_family(&n.path, n.is_dir).into(),
                    icon_color: {
                        let (r, g, b) = peek_tree::icons::icon_color(&n.path, n.is_dir);
                        crate::theme::icon_foreground(r, g, b, light_mode)
                    },
                    has_thumb,
                    thumb: thumb.unwrap_or_default(),
                }
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
                line_number: 0,
            })
            .collect::<Vec<_>>(),
    ))
}

fn project_model(items: Vec<(String, String)>, logos: &Images) -> ModelRc<ProjectItem> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(|(path, display_path)| {
                let logo = logos.get(Path::new(&path)).map(|r| r.to_slint_image());
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
            redo: Vec::new(),
            editor: EditorState::default(),
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
            content_searcher: None,
            content_generation: Arc::new(AtomicU64::new(0)),
            content_searching: false,
            is_image: false,
            current_image: None,
            image_info: String::new(),
            image_dirty: false,
            thumbnail_loader: None,
            tree_viewport: (0, 64),
            thumbnail_generation: 0,
            thumbnails: Images::new(),
            project_logos: Images::new(),
            highlight_cache: HighlightCache::default(),
            preview: false,
        };
        app.open_project(root).unwrap();
        (window, app)
    }

    #[test]
    fn advanced_editor_find_replace_history_save_and_stale_commands() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        fs::write(&file, "\u{feff}niño niño\r\nniño\n").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&file).unwrap();
        let canonical = file.canonicalize().unwrap();
        app.handle(Command::FindAdvanced("niño".into(), true, false, true))
            .unwrap();
        assert_eq!(app.editor.selection(), 0..4);
        app.handle(Command::FindAdvanced("niño".into(), true, false, true))
            .unwrap();
        assert_eq!(app.editor.selection(), 5..9);
        app.handle(Command::Replace(
            "niño".into(),
            "árbol😀".into(),
            true,
            false,
            true,
        ))
        .unwrap();
        assert_eq!(
            app.workspace.buffer.as_ref().unwrap().rope.to_string(),
            "árbol😀 árbol😀\r\nárbol😀\n"
        );
        app.handle(Command::Undo).unwrap();
        assert!(!app.workspace.is_dirty());
        assert_eq!(app.editor.selection(), 5..9);
        app.handle(Command::Redo).unwrap();
        app.handle(Command::Save).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "\u{feff}árbol😀 árbol😀\r\nárbol😀\n"
        );
        app.handle(Command::Undo).unwrap();
        assert!(app.workspace.is_dirty());
        app.handle(Command::EditorAction(
            canonical.clone(),
            "insert".into(),
            "nuevo".into(),
            false,
            false,
        ))
        .unwrap();
        assert!(app.redo.is_empty());
        let before = app.workspace.buffer.as_ref().unwrap().rope.clone();
        app.handle(Command::EditorAction(
            dir.path().join("stale.rs"),
            "paste".into(),
            "incorrecto".into(),
            false,
            false,
        ))
        .unwrap();
        assert_eq!(app.workspace.buffer.as_ref().unwrap().rope, before);
        assert!(app.handle(Command::GoLine("0".into())).is_err());
        app.handle(Command::GoLine("2".into())).unwrap();
        assert_eq!(
            app.editor.position(app.workspace.buffer.as_ref().unwrap()),
            (1, 0)
        );
        app.handle(Command::Replace(
            "nuevo".into(),
            "niño".into(),
            false,
            true,
            false,
        ))
        .unwrap();
        assert!(app.workspace.is_dirty());
        assert_eq!(
            app.workspace.buffer.as_ref().unwrap().rope.to_string(),
            "niño niño\r\nniño\n"
        );
    }

    #[test]
    fn viewport_editing_scrolling_and_large_files_keep_history_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("large.rs");
        fs::write(&file, "let value = 42;\n".repeat(50_000)).unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&file).unwrap();
        app.visible = 40;
        app.handle(Command::EditorScrollTo(1.0)).unwrap();
        assert_eq!(app.top, 49_961);
        app.handle(Command::EditorScroll(3)).unwrap();
        assert_eq!(app.top, 49_961);
        let canonical = file.canonicalize().unwrap();
        app.handle(Command::GoLine("50000".into())).unwrap();
        for _ in 0..120 {
            app.handle(Command::EditorAction(
                canonical.clone(),
                "insert".into(),
                "x".into(),
                false,
                false,
            ))
            .unwrap();
        }
        assert_eq!(app.undo.len(), 100);
        assert!(
            app.editor
                .position(app.workspace.buffer.as_ref().unwrap())
                .1
                >= 120
        );
        app.handle(Command::Undo).unwrap();
        assert_eq!(app.redo.len(), 1);
        app.handle(Command::EditorScroll(-100_000)).unwrap();
        assert_eq!(app.top, 0);
        assert_eq!(
            app.editor
                .position(app.workspace.buffer.as_ref().unwrap())
                .0,
            49_999
        );
    }

    #[test]
    fn content_search_returns_lines_and_rejects_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.rs");
        fs::write(
            &source,
            "first\nNeedle one\nother\nneedle two\nneedle three\nneedle four",
        )
        .unwrap();
        let matches = search_content_file(&source, "needle");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].line, 2);
        assert_eq!(matches[1].preview, "needle two");

        let binary = dir.path().join("binary.dat");
        fs::write(&binary, [0xff, 0x00, 0xfe]).unwrap();
        assert!(search_content_file(&binary, "needle").is_empty());
    }

    #[test]
    fn native_editor_changes_save_and_stale_file_events_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("edit.txt");
        fs::write(&file, "original\r\n").unwrap();
        let (_window, mut app) = fixture(dir.path());
        app.open_file(&file).unwrap();
        let canonical = file.canonicalize().unwrap();
        app.handle(Command::DocumentEdited(
            canonical.clone(),
            "niño😀\r\nsecond\n".into(),
        ))
        .unwrap();
        assert!(app.workspace.is_dirty());
        assert_eq!(fs::read_to_string(&file).unwrap(), "original\r\n");
        app.handle(Command::DocumentEdited(
            dir.path().join("stale.txt"),
            "wrong".into(),
        ))
        .unwrap();
        app.handle(Command::Save).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "niño😀\r\nsecond\n");
        assert!(!app.workspace.is_dirty());
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
        assert!(image_loader::project_logo_raw(
            dir.path(),
            &mut image_loader::ThumbnailCache::new()
        )
        .is_some());

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

    #[test]
    fn open_image_file_updates_viewer_and_info() {
        let dir = tempfile::tempdir().unwrap();
        let img_path = dir.path().join("icon.png");
        let img = image::RgbaImage::from_fn(48, 48, |_, _| image::Rgba([255, 0, 0, 255]));
        img.save(&img_path).unwrap();

        let (_window, mut app) = fixture(dir.path());
        app.handle(Command::OpenFile(img_path.clone())).unwrap();

        assert!(app.is_image);
        assert!(app.current_image.is_some());
        assert!(app.image_info.contains("PNG"));
        assert!(app.image_info.contains("48 × 48 px"));
        assert!(app.workspace.buffer.is_none());
        assert_eq!(
            app.workspace.current_file,
            Some(img_path.canonicalize().unwrap())
        );
    }

    #[test]
    fn viewport_cache_survives_cursor_motion_but_not_undo_branches_or_failed_image_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "let answer = 42;\n").unwrap();
        let (_, mut app) = fixture(dir.path());
        app.open_file(&path).unwrap();
        let canonical = path.canonicalize().unwrap();
        let tokens = |app: &mut Controller| {
            app.highlight_cache.get(
                &app.syntax,
                app.workspace.buffer.as_ref().unwrap(),
                Some(&canonical),
                0,
                40,
                false,
            )
        };
        let first = tokens(&mut app);
        app.handle(Command::EditorAction(
            canonical.clone(),
            "right".into(),
            "".into(),
            false,
            false,
        ))
        .unwrap();
        assert!(Arc::ptr_eq(&first, &tokens(&mut app)));
        app.handle(Command::EditorAction(
            canonical.clone(),
            "insert".into(),
            "a".into(),
            false,
            false,
        ))
        .unwrap();
        let edited = tokens(&mut app);
        app.handle(Command::Undo).unwrap();
        app.handle(Command::EditorAction(
            canonical.clone(),
            "insert".into(),
            "b".into(),
            false,
            false,
        ))
        .unwrap();
        assert_ne!(edited[0].spans, tokens(&mut app)[0].spans);
        app.handle(Command::Revert).unwrap();
        app.handle(Command::Preview(true)).unwrap();
        assert!(app.preview);
        app.open_file(&canonical).unwrap();
        assert!(!app.preview);
        let image_path = dir.path().join("photo.png");
        image::RgbaImage::new(32, 32).save(&image_path).unwrap();
        app.open_file(&image_path).unwrap();
        assert!(app.image_dirty);
        app.publish();
        assert!(!app.image_dirty);
        app.handle(Command::Zoom(1)).unwrap();
        assert!(!app.image_dirty);
        assert!(app.open_file(&dir.path().join("missing.txt")).is_err());
        assert!(app.is_image && app.current_image.is_some());
        app.clear_file();
        assert!(app.current_image.is_none());
    }

    #[test]
    fn stale_thumbnail_batches_cannot_replace_current_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let (_, mut app) = fixture(dir.path());
        app.thumbnail_generation = 2;
        let path = dir.path().join("previous.png");
        let mut images = Images::new();
        images.insert(
            path.clone(),
            image_loader::RawRgbaImage::new(vec![0; 4], 1, 1),
        );
        app.navigation_dirty = false;
        app.handle(Command::ThumbnailsReady(
            1,
            images.clone(),
            Images::new(),
            false,
        ))
        .unwrap();
        assert!(app.thumbnails.is_empty());
        app.handle(Command::ThumbnailsReady(2, images, Images::new(), false))
            .unwrap();
        assert!(app.thumbnails.contains_key(&path));
        assert!(!app.navigation_dirty);
        app.handle(Command::ThumbnailsReady(
            2,
            Images::new(),
            Images::new(),
            false,
        ))
        .unwrap();
        assert!(app.thumbnails.contains_key(&path));
        app.handle(Command::ThumbnailsReady(
            2,
            Images::new(),
            Images::new(),
            true,
        ))
        .unwrap();
        assert!(app.thumbnails.is_empty());
    }

    #[test]
    fn thumbnail_updates_keep_models_and_document_state_and_ignore_replaced_rows() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        let model = ModelRc::new(VecModel::from(vec![FileItem {
            path: "image.png".into(),
            name: "Imagen".into(),
            ..Default::default()
        }]));
        window.set_tree_items(model.clone());
        window.set_recent_projects(ModelRc::new(VecModel::from(vec![ProjectItem {
            path: "root".into(),
            ..Default::default()
        }])));
        window.set_selected_path("main.rs".into());
        window.set_caret_column(7);
        window.set_busy(true);
        let raw = image_loader::RawRgbaImage::new(vec![1, 2, 3, 255], 1, 1);
        apply_image_updates(
            &window,
            vec![(0, "image.png".into(), Some(raw.clone()))],
            vec![(0, "root".into(), Some(raw.clone()))],
        );
        assert_eq!(window.get_tree_items(), model);
        assert!(model.row_data(0).unwrap().has_thumb);
        assert!(window.get_recent_projects().row_data(0).unwrap().has_logo);
        assert_eq!(window.get_selected_path().as_str(), "main.rs");
        assert_eq!(window.get_caret_column(), 7);
        assert!(window.get_busy());
        apply_image_updates(&window, vec![(0, "stale.png".into(), None)], Vec::new());
        assert!(model.row_data(0).unwrap().has_thumb);
        apply_image_updates(&window, vec![(0, "image.png".into(), None)], Vec::new());
        assert!(!model.row_data(0).unwrap().has_thumb);
        let mut images = Images::new();
        images.insert(PathBuf::from("image.png"), raw);
        assert!(image_changes(&images, &images.clone()).is_empty());
    }
}
