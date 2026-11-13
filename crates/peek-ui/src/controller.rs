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

use crate::doc_view::{self, DocLayout, DocViewState};
use crate::editor::{self, EditSnapshot, EditorState};
use crate::image_loader;
use crate::pdf_view::{self, PdfEvent, PdfViewState, PdfWorker};
use crate::sheet_view::SheetViewState;
use crate::thumbnails::{self, Images};
use crate::viewport::HighlightCache;
use crate::{
    CodeLineItem, CodeSpanItem, FileItem, MainWindow, PdfThumbItem, ProjectItem, SearchItem,
};

pub enum Command {
    OpenProject(PathBuf),
    OpenFile(PathBuf),
    OpenAt(PathBuf, usize),
    CloseFile(PathBuf),
    MoveTab(usize, usize),
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
    SelectDocBlock(usize, bool),
    SelectDocAll,
    CopyDocSelection(bool),
    JumpDocBlock(usize),
    StepDocWindow(bool),
    OpenDocLink(String),
    ToggleMarkdownDefaultPreview,
    ToggleSingleFileMode,
    SetWindowsIntegration(bool, bool, bool, bool),
    ApplyWindowsIntegration,
    WindowsIntegrationFinished(std::result::Result<String, String>),
    SelectSheet(usize),
    StepSheetRows(bool),
    StepSheetCols(bool),
    SelectSheetCell(usize, usize),
    CopySheetCell(bool),
    PdfStepPage(i32),
    PdfGoPage(String),
    PdfZoom(i32),
    CopyPdfText,
    Pdf(PdfEvent),
    PdfWheelPage(i32),
    PdfThumbsViewport(usize, usize),
    /// Logical width of the reading area and the screen's pixel ratio.
    ReadingWidth(f32, f32),
    DocZoom(i32),
    ToggleCodeBlock(usize),
    WindowsStatusReady(peek_core::WindowsIntegrationStatus),
}

/// Where the reader was in a file, so switching tabs or reloading keeps the place.
#[derive(Clone, Copy, Default)]
struct ViewMemo {
    top: usize,
    pdf_page: usize,
    pdf_scale: f32,
    pdf_fit_width: bool,
    sheet: (usize, usize, usize),
    doc_window: usize,
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
    doc_view: DocViewState,
    is_docx: bool,
    sheet_view: SheetViewState,
    is_sheet: bool,
    sheet_delimited_text: bool,
    pdf_view: PdfViewState,
    is_pdf: bool,
    standalone_parent: Option<PathBuf>,
    windows_status: peek_core::WindowsIntegrationStatus,
    windows_status_text: String,
    worker_tx: Option<Sender<Command>>,
    reading_width: f32,
    view_memos: HashMap<PathBuf, ViewMemo>,
    pdf_thumbs_dirty: bool,
    last_wheel_page: Option<std::time::Instant>,
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
        let standalone_target = initial.as_ref().filter(|p| p.is_file()).and_then(|p| {
            if !settings.open_single_file_without_project {
                return None;
            }
            let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());
            let in_known_project = settings.open_projects.iter().any(|proj| {
                proj.is_dir() && {
                    let cproj = proj.canonicalize().unwrap_or_else(|_| proj.clone());
                    canonical.starts_with(&cproj)
                }
            });
            (!in_known_project).then_some(canonical)
        });
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
        // The registry is only needed by the settings dialog: query it off the startup path.
        let status_tx = tx.clone();
        std::thread::spawn(move || {
            let status = peek_core::windows_integration::query_status();
            let _ = status_tx.send(Command::WindowsStatusReady(status));
        });
        let pdf_tx = tx.clone();
        let pdf_worker = PdfWorker::spawn(move |event| pdf_tx.send(Command::Pdf(event)).is_ok());
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
            doc_view: DocViewState::default(),
            is_docx: false,
            sheet_view: SheetViewState::default(),
            is_sheet: false,
            sheet_delimited_text: false,
            pdf_view: PdfViewState::default(),
            is_pdf: false,
            standalone_parent: None,
            windows_status: peek_core::WindowsIntegrationStatus::default(),
            windows_status_text: String::new(),
            worker_tx: Some(tx),
            reading_width: 0.0,
            view_memos: HashMap::new(),
            pdf_thumbs_dirty: false,
            last_wheel_page: None,
        };
        app.pdf_view.set_worker(pdf_worker);
        if needs_setup {
            app.status = "Abre un proyecto; quedará guardado en tu lista".into();
            {
                let _ = app.window.upgrade_in_event_loop(move |w| {
                    w.set_path_input(cwd.to_string_lossy().as_ref().into());
                    w.set_show_path(true);
                });
            }
        } else if let Some(file_path) = standalone_target {
            if let Err(e) = app.open_standalone_file(&file_path) {
                app.error(e);
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
            // Background results patch their own rows; they never rebuild the window state.
            let silent = matches!(
                command,
                Command::ThumbnailsReady(..)
                    | Command::PdfThumbsViewport(..)
                    | Command::Pdf(PdfEvent::Thumb { .. })
            );
            let passive = silent
                || matches!(
                    command,
                    Command::Pdf(_) | Command::ReadingWidth(..) | Command::WindowsStatusReady(_)
                );
            if !passive {
                app.set_busy(true);
            }
            if let Err(e) = app.handle(command) {
                app.error(e);
            }
            if silent {
                continue;
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
                    | Command::Pdf(_)
                    | Command::PdfThumbsViewport(..)
                    | Command::ReadingWidth(..)
                    | Command::WindowsStatusReady(_)
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
            self.doc_view.revision = 0;
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
                if self.is_docx || (self.is_sheet && !self.sheet_delimited_text) {
                    self.preview = true;
                } else {
                    self.preview = preview;
                }
                if self.is_sheet && self.preview {
                    self.sheet_view.dirty = true;
                }
                self.refresh_doc_view();
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
                    let index = self.recent_files.iter().position(|p| *p == path);
                    let next = index.and_then(|i| {
                        self.recent_files
                            .get(i + 1)
                            .or_else(|| i.checked_sub(1).and_then(|j| self.recent_files.get(j)))
                            .cloned()
                    });
                    if let Some(next) = next {
                        self.open_file(&next)?;
                    } else {
                        self.clear_file();
                    }
                }
                self.recent_files.retain(|p| p != &path);
                self.view_memos.remove(&path);
                self.navigation_dirty = true;
            }
            Command::MoveTab(from, to) => {
                if from < self.recent_files.len() && from != to {
                    let tab = self.recent_files.remove(from);
                    let to = to.min(self.recent_files.len());
                    self.recent_files.insert(to, tab);
                    self.navigation_dirty = true;
                }
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
                if self.workspace.root_dir.is_some() {
                    directory(&self.tree.root)?;
                    self.tree_nodes = self.tree.visible_nodes();
                    self.start_index();
                    self.refresh_library();
                }
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
                self.doc_view.dirty = true;
                self.persist();
            }
            Command::DocZoom(delta) => {
                let zoom = if delta == 0 {
                    1.0
                } else {
                    self.settings.doc_zoom + 0.1 * delta.signum() as f32
                };
                self.settings.doc_zoom = (zoom.clamp(0.7, 2.5) * 10.0).round() / 10.0;
                self.doc_view.dirty = true;
                self.status = format!("Zoom {:.0} %", self.settings.doc_zoom * 100.0);
                self.persist();
            }
            Command::ReadingWidth(width, pixel_ratio) => {
                if (self.reading_width - width).abs() >= 1.0 {
                    self.reading_width = width;
                    self.doc_view.dirty = true;
                }
                if self.pdf_view.set_viewport(width, pixel_ratio) && self.is_pdf {
                    self.pdf_view.request_render();
                }
            }
            Command::ToggleCodeBlock(index) => self.doc_view.toggle_code_block(index),
            Command::WindowsStatusReady(status) => {
                self.windows_status_text = format_windows_status(&status);
                self.windows_status = status;
            }
            Command::ToggleTheme => {
                self.settings.light_mode = !self.settings.light_mode;
                self.navigation_dirty = true;
                self.doc_view.dirty = true;
                self.persist();
            }
            Command::Panels(show_projects, show_explorer) => {
                self.settings.show_projects = show_projects;
                self.settings.show_explorer = show_explorer;
                self.persist();
            }
            Command::WrapColumns(columns) => {
                let columns = columns.max(4);
                if self.wrap_columns != columns {
                    self.wrap_columns = columns;
                    self.top_fragment = 0;
                }
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
                if self.is_pdf {
                    let summary = self.pdf_view.find(&query, forward, case);
                    self.pdf_view.request_render();
                    self.status = summary.clone();
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.set_find_summary(summary.into()));
                } else if self.is_sheet && self.preview {
                    let summary = self.sheet_view.find(&query, forward, case);
                    self.status = summary.clone();
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.set_find_summary(summary.into()));
                } else if self.preview && self.is_markdown_active() {
                    let summary = self.doc_view.find(&query, forward, case);
                    self.status = summary.clone();
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.set_find_summary(summary.into()));
                } else {
                    self.find_advanced(&query, forward, case, whole)?;
                }
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
            Command::SelectDocBlock(index, extend) => {
                self.doc_view.select_block(index, extend);
            }
            Command::SelectDocAll => {
                self.doc_view.select_all();
            }
            Command::CopyDocSelection(all) => {
                if all {
                    self.doc_view.select_all();
                }
                let text = self.doc_view.selected_text();
                if !text.is_empty() {
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.invoke_write_clipboard(text.into()));
                    self.status = "Texto copiado al portapapeles".into();
                }
            }
            Command::JumpDocBlock(block_index) => {
                self.doc_view.jump_to_block(block_index);
            }
            Command::StepDocWindow(forward) => {
                self.doc_view.step_window(forward);
            }
            Command::OpenDocLink(url) => {
                match doc_view::classify_link(self.workspace.current_file.as_deref(), &url) {
                    Some(doc_view::LinkTarget::ExternalUrl(target)) => {
                        peek_core::windows_integration::open_external_url(&target)?;
                        self.status = "Enlace abierto en el navegador".into();
                    }
                    Some(doc_view::LinkTarget::LocalFile(path)) => {
                        if self.workspace.root_dir.is_none() {
                            self.open_standalone_file(&path)?;
                        } else {
                            self.open_file(&path)?;
                        }
                    }
                    None => {
                        self.status = "Enlace no compatible o fuera del alcance local".into();
                    }
                }
            }
            Command::ToggleMarkdownDefaultPreview => {
                self.settings.markdown_default_preview = !self.settings.markdown_default_preview;
                self.persist();
            }
            Command::ToggleSingleFileMode => {
                self.settings.open_single_file_without_project =
                    !self.settings.open_single_file_without_project;
                self.persist();
            }
            Command::SetWindowsIntegration(context_menu, open_md, open_code, open_docs) => {
                self.settings.windows_context_menu = context_menu;
                self.settings.windows_open_with_markdown = open_md;
                self.settings.windows_open_with_code = open_code;
                self.settings.windows_open_with_docs = open_docs;
                self.persist();
            }
            Command::SelectSheet(index) => {
                self.sheet_view.select_sheet(index);
            }
            Command::StepSheetRows(forward) => {
                self.sheet_view.step_rows(forward);
            }
            Command::StepSheetCols(forward) => {
                self.sheet_view.step_cols(forward);
            }
            Command::SelectSheetCell(row, col) => {
                self.sheet_view.select_cell(row, col);
            }
            Command::CopySheetCell(full_row) => {
                let text = if full_row {
                    self.sheet_view.selected_row_tsv()
                } else {
                    let cell = self.sheet_view.selected_text();
                    if cell.is_empty() {
                        self.sheet_view.selected_row_tsv()
                    } else {
                        cell
                    }
                };
                if !text.is_empty() {
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.invoke_write_clipboard(text.into()));
                    self.status = "Celda copiada al portapapeles".into();
                }
            }
            Command::PdfStepPage(delta) => {
                if self.pdf_view.step_page(delta as isize) {
                    self.pdf_view.request_render();
                }
            }
            Command::PdfGoPage(input) => {
                let page: usize = input
                    .trim()
                    .parse()
                    .context("Escribe un número de página válido")?;
                if self.pdf_view.go_page(page)? {
                    self.pdf_view.request_render();
                }
                let _ = self.window.upgrade_in_event_loop(|w| {
                    w.set_show_goto(false);
                    w.invoke_focus_content();
                });
            }
            Command::PdfZoom(delta) => {
                if self.pdf_view.adjust_zoom(delta) {
                    self.pdf_view.request_render();
                }
            }
            Command::CopyPdfText => {
                let text = self.pdf_view.current_page_text();
                if text.trim().is_empty() {
                    self.status = "Esta página PDF no contiene texto extraíble".into();
                } else {
                    let _ = self
                        .window
                        .upgrade_in_event_loop(move |w| w.invoke_write_clipboard(text.into()));
                    self.status = "Texto de la página copiado al portapapeles".into();
                }
            }
            Command::PdfWheelPage(delta) => {
                // One wheel gesture sends a burst of events: turn a single page per burst.
                let now = std::time::Instant::now();
                if self
                    .last_wheel_page
                    .is_none_or(|last| now.duration_since(last) > Duration::from_millis(220))
                    && self.pdf_view.step_page(delta.signum() as isize)
                {
                    self.last_wheel_page = Some(now);
                    self.pdf_view.request_render();
                }
            }
            Command::PdfThumbsViewport(first, count) => {
                self.pdf_view.request_thumbs(first, count.clamp(1, 64));
            }
            Command::Pdf(event) => match event {
                PdfEvent::Page {
                    generation,
                    page,
                    scale_pct,
                    result,
                } if generation == self.pdf_view.generation => match result {
                    Ok(raw) => self.pdf_view.insert_cached_page(page, scale_pct, raw),
                    Err(err) => {
                        self.pdf_view.loading = false;
                        self.pdf_view.dirty = true;
                        bail!("{err}");
                    }
                },
                PdfEvent::Thumb {
                    generation,
                    page,
                    image,
                } if generation == self.pdf_view.generation => {
                    let evicted = self.pdf_view.insert_thumb(page, image.clone());
                    let _ = self.window.upgrade_in_event_loop(move |w| {
                        let model = w.get_pdf_thumbs();
                        let set = |row: usize, image: Option<&image_loader::RawRgbaImage>| {
                            if row < model.row_count() {
                                model.set_row_data(
                                    row,
                                    PdfThumbItem {
                                        page: row as i32,
                                        has_image: image.is_some(),
                                        image: image_loader::RawRgbaImage::to_slint_or_default(
                                            image,
                                        ),
                                    },
                                );
                            }
                        };
                        if let Some(old) = evicted {
                            set(old, None);
                        }
                        set(page, Some(&image));
                    });
                }
                PdfEvent::Search {
                    generation,
                    id,
                    pages,
                    done,
                } if generation == self.pdf_view.generation => {
                    if let Some(summary) = self.pdf_view.apply_search(id, pages, done) {
                        self.pdf_view.request_render();
                        self.status = summary.clone();
                        let _ = self
                            .window
                            .upgrade_in_event_loop(move |w| w.set_find_summary(summary.into()));
                    }
                }
                _ => {}
            },
            Command::ApplyWindowsIntegration => {
                let options = peek_core::WindowsIntegrationOptions::from(&self.settings);
                if let Some(tx) = self.worker_tx.clone() {
                    self.windows_status_text = "Aplicando integración en Windows…".into();
                    std::thread::spawn(move || {
                        let result = peek_core::windows_integration::apply_integration(&options)
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(Command::WindowsIntegrationFinished(result));
                    });
                } else {
                    let msg = peek_core::windows_integration::apply_integration(&options)?;
                    self.windows_status = peek_core::windows_integration::query_status();
                    self.windows_status_text = format_windows_status(&self.windows_status);
                    self.status = msg;
                }
            }
            Command::WindowsIntegrationFinished(result) => match result {
                Ok(message) => {
                    self.windows_status = peek_core::windows_integration::query_status();
                    self.windows_status_text = format_windows_status(&self.windows_status);
                    self.status = message;
                }
                Err(error) => {
                    self.windows_status = peek_core::windows_integration::query_status();
                    self.windows_status_text = format_windows_status(&self.windows_status);
                    anyhow::bail!("{error}");
                }
            },
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

    fn open_standalone_file(&mut self, path: &Path) -> Result<()> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let parent = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        self.workspace.root_dir = None;
        self.standalone_parent = (!parent.as_os_str().is_empty()).then_some(parent);
        self.tree_nodes.clear();
        self.paths.clear();
        self.index_generation.fetch_add(1, Ordering::Relaxed);
        self.indexing = false;
        self.active_search = None;
        self.content_generation.fetch_add(1, Ordering::Relaxed);
        self.content_searching = false;
        self.open_file(&canonical)?;
        self.navigation_dirty = true;
        Ok(())
    }

    fn open_project(&mut self, path: &Path) -> Result<()> {
        let root = directory(path)?;
        if self.workspace.root_dir.as_ref() == Some(&root) {
            return Ok(());
        }
        let preserved_standalone = if self.workspace.root_dir.is_none() {
            self.workspace
                .current_file
                .clone()
                .filter(|file| file.starts_with(&root) && file.is_file())
        } else {
            None
        };
        self.standalone_parent = None;
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
        if let Some(file) = preserved_standalone {
            if let Err(e) = self.open_file(&file) {
                self.error(e);
            }
        } else if let Some(session) = session {
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
        self.doc_view.clear();
        self.sheet_view.clear();
        self.pdf_view.clear();
        self.is_docx = false;
        self.is_sheet = false;
        self.sheet_delimited_text = false;
        self.is_pdf = false;
        self.pdf_thumbs_dirty = true;
        self.preview = false;
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
            w.set_markdown_file(false);
            w.set_doc_readonly(false);
            w.set_is_sheet(false);
            w.set_is_pdf(false);
            w.set_preview(false);
            w.set_doc_blocks(ModelRc::default());
            w.set_doc_headings(ModelRc::default());
            w.set_sheet_rows(ModelRc::default());
            w.set_sheet_headers(ModelRc::default());
            w.set_sheet_tabs(ModelRc::default());
            w.set_pdf_image(slint::Image::default());
            w.set_pdf_thumbs(ModelRc::default());
        });
    }

    fn record_opened_file(&mut self, canonical: &Path) {
        self.standalone_parent = if self.workspace.root_dir.is_some() {
            None
        } else {
            canonical
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(Path::to_path_buf)
        };
        self.workspace.current_file = Some(canonical.to_path_buf());
        self.stamp = file_stamp(canonical);
        self.content_width = 0.0;
        self.last_find.clear();
        self.navigation_dirty = true;
        if !self.recent_files.iter().any(|p| p == canonical) {
            self.recent_files.push(canonical.to_path_buf());
        }
        if self.workspace.root_dir.is_some() {
            let mut parent = canonical.parent();
            while let Some(dir) = parent.filter(|p| p.starts_with(&self.tree.root)) {
                self.tree.expanded_dirs.insert(dir.to_path_buf());
                parent = dir.parent();
            }
            self.tree_nodes = self.tree.visible_nodes();
        }
    }

    fn reset_binary_modes(&mut self, keep_pdf: bool) {
        self.is_image = false;
        self.current_image = None;
        self.image_dirty = true;
        self.image_info.clear();
        self.is_docx = false;
        self.is_sheet = false;
        self.sheet_delimited_text = false;
        self.is_pdf = false;
        self.sheet_view.clear();
        self.doc_view.clear();
        self.pdf_thumbs_dirty = true;
        if !keep_pdf {
            self.pdf_view.clear();
        }
    }

    /// Remembers where the reader is in the current file before another one replaces it.
    fn remember_view(&mut self) {
        let Some(path) = self.workspace.current_file.clone() else {
            return;
        };
        if self.view_memos.len() >= 64 && !self.view_memos.contains_key(&path) {
            self.view_memos.clear();
        }
        self.view_memos.insert(
            path,
            ViewMemo {
                top: self.top,
                pdf_page: self.pdf_view.current_page,
                pdf_scale: self.pdf_view.scale,
                pdf_fit_width: self.pdf_view.fit_width,
                sheet: (
                    self.sheet_view.active_sheet,
                    self.sheet_view.row_start,
                    self.sheet_view.col_start,
                ),
                doc_window: self.doc_view.window_start,
            },
        );
    }

    /// Installs a freshly loaded document as the active tab. Loading already succeeded,
    /// so nothing here can fail and leave the previous file half-replaced.
    fn begin_document(&mut self, canonical: &Path, buffer: Option<TextBuffer>, preview: bool) {
        self.highlight_cache.invalidate();
        self.original = buffer.clone();
        self.workspace.buffer = buffer;
        self.undo.clear();
        self.redo.clear();
        self.editor = EditorState::default();
        self.top = 0;
        self.top_fragment = 0;
        self.selected = 0;
        self.preview = preview;
        self.record_opened_file(canonical);
        self.reset_horizontal();
    }

    fn open_file(&mut self, path: &Path) -> Result<()> {
        self.remember_view();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let memo = self.view_memos.get(&canonical).copied();

        if image_loader::is_image_file(path) {
            let details = image_loader::load_image_full(path)?;
            self.reset_binary_modes(false);
            self.is_image = true;
            self.current_image = Some(details.raw);
            self.image_info = details.summary.clone();
            self.begin_document(&canonical, None, false);
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
            return Ok(());
        }

        if pdf_view::is_pdf_extension(path) {
            self.pdf_view.open_file(path)?;
            self.reset_binary_modes(true);
            self.is_pdf = true;
            self.begin_document(&canonical, None, false);
            if let Some(memo) = memo {
                self.pdf_view.set_page(memo.pdf_page);
                self.pdf_view.scale = memo.pdf_scale;
                self.pdf_view.fit_width = memo.pdf_fit_width;
            }
            self.pdf_view.request_render();
            self.pdf_view.request_thumbs(self.pdf_view.current_page, 12);
            self.status = "PDF · RePág/AvPág página · Ctrl+F buscar · Ctrl+G ir a página".into();
            return Ok(());
        }

        if peek_syntax::is_docx_extension(path) {
            let docx = peek_syntax::load_docx_file(path)?;
            self.reset_binary_modes(false);
            self.is_docx = true;
            self.begin_document(&canonical, None, true);
            self.doc_view.set_docx_document(docx, 1);
            if let Some(memo) = memo {
                self.doc_view.window_start = memo.doc_window;
            }
            self.status = "Documento Word · Ctrl+F buscar · Ctrl+C copiar".into();
            return Ok(());
        }

        if peek_syntax::is_spreadsheet_extension(path) {
            let delimited = peek_syntax::is_delimited_text(path);
            // CSV/TSV share one read: the table is parsed from the text buffer in memory.
            let (sheet_doc, buffer) = if delimited {
                let buffer = TextBuffer::from_file(path)?;
                let doc = peek_syntax::SpreadsheetDocument::from_buffer(path, &buffer)?;
                (doc, Some(buffer))
            } else {
                (peek_syntax::SpreadsheetDocument::from_file(path)?, None)
            };
            self.reset_binary_modes(false);
            self.is_sheet = true;
            self.sheet_delimited_text = delimited;
            self.begin_document(&canonical, buffer, true);
            self.sheet_view.set_document(sheet_doc);
            if let Some(memo) = memo {
                let (sheet, row, col) = memo.sheet;
                self.sheet_view.restore(sheet, row, col);
            }
            self.status = "Hoja de cálculo · Ctrl+F buscar · Ctrl+C copiar celda".into();
            return Ok(());
        }

        if let Some(message) = unsupported_document(path) {
            bail!(message);
        }

        // Read first: a missing/binary/invalid file must not clear the current buffer.
        let buffer = TextBuffer::from_file(path)?;
        let canonical = buffer.path.clone().context("No se pudo resolver la ruta")?;
        self.reset_binary_modes(false);
        self.begin_document(&canonical, Some(buffer), false);
        self.status = "Ctrl+S guardar · Ctrl+P buscar archivos".into();
        self.sync_document();
        if let Some(memo) = memo {
            self.top = memo.top.min(self.max_scroll());
            self.doc_view.window_start = memo.doc_window.min(
                self.doc_view
                    .document
                    .as_ref()
                    .map_or(0, |doc| doc.blocks.len().saturating_sub(1)),
            );
        }
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

    fn is_markdown_file_ext(&self) -> bool {
        self.workspace
            .current_file
            .as_deref()
            .and_then(|path| self.syntax.find_extension(path))
            .is_some_and(|extension| extension.is_markdown())
    }

    fn is_markdown_active(&self) -> bool {
        self.is_docx || self.is_markdown_file_ext()
    }

    fn refresh_doc_view(&mut self) {
        self.doc_view.dirty = true;
        if self.is_docx || !self.preview || !self.is_markdown_file_ext() {
            // Leaving the preview keeps the parsed document for the way back.
            return;
        }
        let Some(buffer) = &self.workspace.buffer else {
            return;
        };
        // Edits bump the buffer revision; undo branches reset `doc_view.revision` to 0.
        let revision = buffer.revision + 1;
        if self.doc_view.document.is_some() && self.doc_view.revision == revision {
            return;
        }
        let source = buffer.rope.to_string();
        let root = self
            .workspace
            .root_dir
            .as_deref()
            .or(self.standalone_parent.as_deref());
        self.doc_view.ensure_markdown_parsed(
            &source,
            revision,
            self.workspace.current_file.as_deref(),
            root,
            &self.syntax,
        );
    }

    fn sync_document(&mut self) {
        let markdown = self.is_markdown_active();
        self.preview = markdown && self.settings.markdown_default_preview;
        self.refresh_doc_view();
        let preview = self.preview;
        let _ = self.window.upgrade_in_event_loop(move |w| {
            w.set_is_image(false);
            w.set_document_text("".into());
            w.set_editor_selection_start(-1);
            w.set_editor_selection_end(-1);
            w.set_markdown_file(markdown);
            w.set_preview(preview);
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
        let active_tab = self
            .workspace
            .current_file
            .as_ref()
            .and_then(|f| self.recent_files.iter().position(|p| p == f))
            .map_or(-1, |i| i as i32);
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
        let status = if self.is_image
            || self.is_docx
            || self.is_pdf
            || (self.is_sheet && !self.sheet_delimited_text)
        {
            self.status.clone()
        } else if let Some(b) = &self.workspace.buffer {
            format!(
                "Línea {} / {}   {}{}   {}   {}",
                selected,
                total,
                b.encoding.label(),
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
        // Only text buffers have a language; documents must not load the grammar set.
        let language = self
            .workspace
            .current_file
            .as_deref()
            .filter(|_| self.workspace.buffer.is_some())
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
        let markdown = self.is_markdown_active();
        let doc_readonly = self.is_docx || (self.is_sheet && !self.sheet_delimited_text);
        let is_sheet = self.is_sheet;
        let is_pdf = self.is_pdf;
        let preview = self.preview;
        let standalone_parent_path = self
            .standalone_parent
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let markdown_default_preview = self.settings.markdown_default_preview;
        let single_file_mode = self.settings.open_single_file_without_project;
        let win_supported = self.windows_status.supported;
        let win_context_menu = self.settings.windows_context_menu;
        let win_open_md = self.settings.windows_open_with_markdown;
        let win_open_code = self.settings.windows_open_with_code;
        let win_open_docs = self.settings.windows_open_with_docs;
        let win_status_text = self.windows_status_text.clone();
        let doc_zoom = self.settings.doc_zoom;
        let doc_active = preview && markdown;
        let doc_full = std::mem::take(&mut self.doc_view.dirty);
        let doc_flags = std::mem::take(&mut self.doc_view.flags_dirty) && !doc_full && doc_active;
        let doc_update = doc_full.then(|| {
            if doc_active {
                self.doc_view.prepare_models(DocLayout {
                    width: self.reading_width,
                    font: ui_font * doc_zoom,
                    zoom: doc_zoom,
                })
            } else {
                doc_view::PreparedDocModels {
                    blocks: Vec::new(),
                    headings: Vec::new(),
                    total_blocks: 0,
                    window_start: 0,
                    has_more_before: false,
                    has_more_after: false,
                }
            }
        });
        let doc_flag_update = doc_flags.then(|| self.doc_view.window_flags());
        let doc_scroll = self
            .doc_view
            .scroll_target
            .take()
            .filter(|_| doc_active)
            .map(|block| block as i32);
        let sheet_active = is_sheet && preview;
        let sheet_full = std::mem::take(&mut self.sheet_view.dirty);
        let sheet_row_updates = self.sheet_view.take_row_updates();
        let sheet_update = sheet_full.then(|| {
            if sheet_active {
                self.sheet_view.prepare_models()
            } else {
                crate::sheet_view::PreparedSheetModels::default()
            }
        });
        let sheet_cell_ref = self.sheet_view.selected_ref();
        let sheet_cell_text = self.sheet_view.selected_text();
        let pdf_update = std::mem::take(&mut self.pdf_view.dirty).then(|| {
            let (width, height) = self.pdf_view.display_size();
            (
                self.pdf_view.current_page as i32,
                self.pdf_view.total_pages as i32,
                self.pdf_view.scale_percent() as i32,
                self.pdf_view.loading,
                self.pdf_view.display_image().cloned().filter(|_| is_pdf),
                (width, height, self.pdf_view.fit_width),
            )
        });
        let pdf_thumbs = std::mem::take(&mut self.pdf_thumbs_dirty).then(|| {
            let pages = if is_pdf { self.pdf_view.total_pages } else { 0 };
            let thumbs: Vec<_> = (0..pages)
                .map(|page| self.pdf_view.thumb(page).cloned())
                .collect();
            (thumbs, self.pdf_view.thumb_height())
        });
        let mut rows = Vec::new();
        if let Some(buffer) = self
            .workspace
            .buffer
            .as_ref()
            .filter(|_| preview && !markdown && !is_sheet && !is_pdf)
        {
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
            w.set_markdown_file(markdown);
            w.set_doc_readonly(doc_readonly);
            w.set_is_sheet(is_sheet);
            w.set_is_pdf(is_pdf);
            w.set_preview(preview);
            w.set_standalone_parent_path(standalone_parent_path.into());
            w.set_markdown_default_preview(markdown_default_preview);
            w.set_open_single_file_without_project(single_file_mode);
            w.set_windows_integration_supported(win_supported);
            w.set_windows_context_menu(win_context_menu);
            w.set_windows_open_with_markdown(win_open_md);
            w.set_windows_open_with_code(win_open_code);
            w.set_windows_open_with_docs(win_open_docs);
            w.set_windows_integration_status(win_status_text.into());
            w.set_doc_zoom(doc_zoom);
            if let Some(flags) = doc_flag_update {
                let model = w.get_doc_blocks();
                for (row, (selected, highlighted)) in flags.into_iter().enumerate() {
                    if let Some(mut item) = model.row_data(row) {
                        if item.selected != selected || item.highlighted != highlighted {
                            item.selected = selected;
                            item.highlighted = highlighted;
                            model.set_row_data(row, item);
                        }
                    }
                }
            }
            if let Some(prepared) = doc_update {
                let total_blocks = prepared.total_blocks;
                let win_start = prepared.window_start;
                let has_before = prepared.has_more_before;
                let has_after = prepared.has_more_after;
                let (blocks, headings) = prepared.into_slint(light_mode);
                w.set_doc_blocks(ModelRc::new(VecModel::from(blocks)));
                w.set_doc_headings(ModelRc::new(VecModel::from(headings)));
                w.set_doc_total_blocks(total_blocks);
                w.set_doc_window_start(win_start);
                w.set_doc_has_more_before(has_before);
                w.set_doc_has_more_after(has_after);
            }
            if let Some(block) = doc_scroll {
                w.set_doc_scroll_target(block);
                w.set_doc_scroll_seq(w.get_doc_scroll_seq().wrapping_add(1));
            }
            if let Some(prepared) = sheet_update {
                let total_rows = prepared.total_rows;
                let total_cols = prepared.total_cols;
                let row_start = prepared.row_start;
                let col_start = prepared.col_start;
                let has_prev_rows = prepared.has_prev_rows;
                let has_next_rows = prepared.has_next_rows;
                let has_prev_cols = prepared.has_prev_cols;
                let has_next_cols = prepared.has_next_cols;
                let active_sheet = prepared.active_sheet;
                let truncated = prepared.truncated;
                let (tabs, headers, rows) = prepared.into_slint();
                w.set_sheet_tabs(ModelRc::new(VecModel::from(tabs)));
                w.set_active_sheet(active_sheet);
                w.set_sheet_headers(ModelRc::new(VecModel::from(headers)));
                w.set_sheet_rows(ModelRc::new(VecModel::from(rows)));
                w.set_sheet_total_rows(total_rows);
                w.set_sheet_total_cols(total_cols);
                w.set_sheet_row_start(row_start);
                w.set_sheet_col_start(col_start);
                w.set_sheet_has_prev_rows(has_prev_rows);
                w.set_sheet_has_next_rows(has_next_rows);
                w.set_sheet_has_prev_cols(has_prev_cols);
                w.set_sheet_has_next_cols(has_next_cols);
                w.set_sheet_truncated(truncated);
            } else if !sheet_row_updates.is_empty() {
                let model = w.get_sheet_rows();
                for (offset, row) in sheet_row_updates {
                    if offset < model.row_count() {
                        model.set_row_data(offset, row.into_slint());
                    }
                }
            }
            w.set_sheet_cell_ref(sheet_cell_ref.into());
            w.set_sheet_cell_text(sheet_cell_text.into());
            if let Some((page, total_pages, zoom_pct, loading, img, (width, height, fit))) =
                pdf_update
            {
                w.set_pdf_page(page);
                w.set_pdf_total_pages(total_pages);
                w.set_pdf_zoom_percent(zoom_pct);
                w.set_pdf_loading(loading);
                w.set_pdf_fit_width(fit);
                w.set_pdf_page_width(width);
                w.set_pdf_page_height(height);
                w.set_pdf_image(image_loader::RawRgbaImage::to_slint_or_default(
                    img.as_ref(),
                ));
            }
            if let Some((thumbs, thumb_height)) = pdf_thumbs {
                w.set_pdf_thumb_height(thumb_height);
                w.set_pdf_thumbs(ModelRc::new(VecModel::from(
                    thumbs
                        .into_iter()
                        .enumerate()
                        .map(|(page, image)| PdfThumbItem {
                            page: page as i32,
                            has_image: image.is_some(),
                            image: image_loader::RawRgbaImage::to_slint_or_default(image.as_ref()),
                        })
                        .collect::<Vec<_>>(),
                )));
            }
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
                w.set_active_tab(active_tab);
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

/// Office formats CodePeek recognizes but cannot render yet: say so instead of
/// failing with a generic "binary file" error.
fn unsupported_document(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let hint = match ext.as_str() {
        "doc" | "dot" => "Word 97-2003 (.doc). Guárdalo como .docx para verlo aquí",
        "ppt" | "pptx" | "odp" => "presentaciones (PowerPoint/Impress)",
        "odt" => "OpenDocument de texto (.odt). Guárdalo como .docx para verlo aquí",
        _ => return None,
    };
    Some(format!("CodePeek todavía no puede mostrar {hint}."))
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

fn format_windows_status(status: &peek_core::WindowsIntegrationStatus) -> String {
    if !status.supported {
        return "Disponible solo en Windows".into();
    }
    if !status.installed {
        return "Modo portable: al aplicar se instalará en AppData\\Local\\Programs\\CodePeek"
            .into();
    }
    let mut parts = Vec::new();
    if status.context_menu {
        parts.push("Menú contextual");
    }
    if status.open_with_markdown {
        parts.push("Abrir con (.md)");
    }
    if status.open_with_docs {
        parts.push("Abrir con (docs)");
    }
    if status.open_with_code {
        parts.push("Abrir con (código)");
    }
    if parts.is_empty() {
        "Instalado en AppData, sin asociaciones activas".into()
    } else {
        format!("Activo en Windows: {}", parts.join(" · "))
    }
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
            doc_view: DocViewState::default(),
            is_docx: false,
            sheet_view: SheetViewState::default(),
            is_sheet: false,
            sheet_delimited_text: false,
            pdf_view: PdfViewState::default(),
            is_pdf: false,
            standalone_parent: None,
            windows_status: peek_core::WindowsIntegrationStatus::default(),
            windows_status_text: String::new(),
            worker_tx: None,
            reading_width: 0.0,
            view_memos: HashMap::new(),
            pdf_thumbs_dirty: false,
            last_wheel_page: None,
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
    fn tabs_keep_their_order_and_can_be_moved() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = ["a.txt", "b.txt", "c.txt"]
            .iter()
            .map(|n| {
                let p = dir.path().join(n);
                fs::write(&p, n).unwrap();
                fs::canonicalize(&p).unwrap()
            })
            .collect();
        let (_window, mut app) = fixture(dir.path());
        for p in &paths {
            app.open_file(p).unwrap();
        }
        app.open_file(&paths[0]).unwrap();
        assert_eq!(app.recent_files, paths);
        app.handle(Command::MoveTab(0, 2)).unwrap();
        assert_eq!(
            app.recent_files,
            [&paths[1], &paths[2], &paths[0]].map(Clone::clone)
        );
        app.handle(Command::OpenFile(paths[2].clone())).unwrap();
        app.handle(Command::CloseFile(paths[2].clone())).unwrap();
        assert_eq!(app.workspace.current_file.as_ref(), Some(&paths[0]));
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

    #[test]
    fn standalone_file_mode_skips_parent_index_and_can_promote_parent_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notas.md");
        let sibling = dir.path().join("otro.txt");
        fs::write(&file, "# Título\n\nContenido de prueba\n").unwrap();
        fs::write(&sibling, "ignorar").unwrap();
        let (_, mut app) = fixture(dir.path());
        // Reset fixture back to clean standalone file open
        app.settings.open_projects.clear();
        app.open_standalone_file(&file).unwrap();
        assert!(app.workspace.root_dir.is_none());
        assert!(app.standalone_parent.is_some());
        assert!(app.tree_nodes.is_empty());
        assert!(app.paths.is_empty());
        assert!(app.settings.open_projects.is_empty());
        assert!(app.preview);
        assert!(app.doc_view.document.is_some());

        let parent = app.standalone_parent.clone().unwrap();
        app.open_project(&parent).unwrap();
        assert_eq!(
            app.workspace.root_dir.as_ref().unwrap(),
            &parent.canonicalize().unwrap()
        );
        assert!(app.standalone_parent.is_none());
        assert_eq!(
            app.workspace.current_file.as_ref().unwrap(),
            &file.canonicalize().unwrap()
        );
        assert!(!app.tree_nodes.is_empty());
    }

    #[test]
    fn markdown_opens_in_rich_preview_toggles_to_editor_and_supports_search() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("README.md");
        fs::write(
            &md,
            "# Introducción\n\nTexto con **negrita**.\n\n## Detalles\n\n| Col | Val |\n| --- | --- |\n| X | 42 |\n",
        )
        .unwrap();
        let (_, mut app) = fixture(dir.path());
        app.open_file(&md).unwrap();
        assert!(app.preview);
        assert_eq!(app.doc_view.document.as_ref().unwrap().headings.len(), 2);
        app.handle(Command::FindAdvanced("Detalles".into(), true, false, false))
            .unwrap();
        assert!(app.status.contains("de"));

        app.handle(Command::Preview(false)).unwrap();
        assert!(!app.preview);
        // The parsed document survives the toggle: coming back must not parse again.
        let revision = app.doc_view.revision;
        app.doc_view.window_start = 1;
        app.handle(Command::Preview(true)).unwrap();
        assert!(app.preview);
        assert_eq!(app.doc_view.revision, revision);
        assert_eq!(app.doc_view.window_start, 1);

        // An edit invalidates it.
        app.handle(Command::Preview(false)).unwrap();
        app.handle(Command::EditorAction(
            md.canonicalize().unwrap(),
            "insert".into(),
            "Ñ".into(),
            false,
            false,
        ))
        .unwrap();
        app.handle(Command::Preview(true)).unwrap();
        assert_ne!(app.doc_view.revision, revision);
        assert_eq!(app.doc_view.window_start, 0);
    }

    #[test]
    fn legacy_encodings_open_with_accents_and_save_back_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let text = "Año: 2025\r\nNiño canción\r\n";
        let (latin, _, _) = encoding_rs::WINDOWS_1252.encode(text);
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let (_, mut app) = fixture(dir.path());
        for (name, bytes, label) in [
            ("latin.txt", latin.to_vec(), "Windows-1252"),
            ("utf16.txt", utf16, "UTF-16 LE"),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, &bytes).unwrap();
            app.open_file(&path).unwrap();
            let buffer = app.workspace.buffer.as_ref().unwrap();
            assert_eq!(buffer.encoding.label(), label);
            assert_eq!(buffer.line_trimmed(0).unwrap(), "Año: 2025");
            assert_eq!(buffer.line_trimmed(1).unwrap(), "Niño canción");

            // Editing and saving keeps the original encoding, BOM and line endings.
            let canonical = app.workspace.current_file.clone().unwrap();
            app.handle(Command::EditorAction(
                canonical,
                "insert".into(),
                "ñ".into(),
                false,
                false,
            ))
            .unwrap();
            app.handle(Command::Save).unwrap();
            let saved = fs::read(&path).unwrap();
            assert_eq!(
                saved.len(),
                bytes.len() + if label == "UTF-16 LE" { 2 } else { 1 }
            );
            let reopened = TextBuffer::from_file(&path).unwrap();
            assert_eq!(reopened.encoding.label(), label);
            assert_eq!(reopened.line_trimmed(0).unwrap(), "ñAño: 2025");
            assert_eq!(reopened.line_ending, peek_core::LineEnding::Crlf);
        }

        // Characters the legacy code page cannot hold are refused, never written as "?".
        let path = dir.path().join("latin.txt");
        app.open_file(&path).unwrap();
        let before = fs::read(&path).unwrap();
        app.handle(Command::EditorAction(
            app.workspace.current_file.clone().unwrap(),
            "insert".into(),
            "→".into(),
            false,
            false,
        ))
        .unwrap();
        assert!(app.handle(Command::Save).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);

        // Real binaries are still rejected, and legacy Office files explain why.
        let binary = dir.path().join("datos.bin");
        fs::write(&binary, [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
        assert!(app.open_file(&binary).is_err());
        let doc = dir.path().join("viejo.doc");
        fs::write(&doc, [0xD0u8, 0xCF, 0x11, 0xE0]).unwrap();
        let error = app.open_file(&doc).unwrap_err().to_string();
        assert!(error.contains("Word 97-2003"), "{error}");
    }

    #[test]
    fn tabs_keep_each_documents_place_and_external_files_join_them() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("manual.pdf");
        fs::write(&pdf, crate::pdf_view::test_pdf(&["Uno", "Dos", "Tres"])).unwrap();
        let csv = dir.path().join("datos.csv");
        let rows: String = (0..400).map(|i| format!("fila {i},{i}\n")).collect();
        fs::write(&csv, rows).unwrap();
        // A file outside the project, as sent by a second launch of CodePeek.
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("externo.txt");
        fs::write(&external, "Señal externa\n").unwrap();

        let (_, mut app) = fixture(dir.path());
        app.handle(Command::OpenFile(csv.clone())).unwrap();
        app.handle(Command::StepSheetRows(true)).unwrap();
        app.handle(Command::StepSheetRows(true)).unwrap();
        assert_eq!(app.sheet_view.row_start, 100);

        app.handle(Command::OpenFile(pdf.clone())).unwrap();
        app.handle(Command::PdfStepPage(2)).unwrap();
        app.handle(Command::PdfZoom(1)).unwrap();
        assert_eq!(app.pdf_view.current_page, 2);

        app.handle(Command::OpenFile(external.clone())).unwrap();
        assert_eq!(
            app.workspace
                .buffer
                .as_ref()
                .unwrap()
                .line_trimmed(0)
                .unwrap(),
            "Señal externa"
        );
        // The project stays open and all three documents are tabs.
        assert!(app.workspace.root_dir.is_some());
        assert_eq!(app.recent_files.len(), 3);

        let csv = csv.canonicalize().unwrap();
        let pdf = pdf.canonicalize().unwrap();
        app.handle(Command::OpenFile(csv.clone())).unwrap();
        assert!(app.is_sheet);
        assert_eq!(app.sheet_view.row_start, 100);
        app.handle(Command::OpenFile(pdf.clone())).unwrap();
        assert!(app.is_pdf);
        assert_eq!(app.pdf_view.current_page, 2);
        assert!(!app.pdf_view.fit_width);

        // Closing a tab forgets its place.
        app.handle(Command::CloseFile(pdf.clone())).unwrap();
        assert!(!app.view_memos.contains_key(&pdf));
        assert_eq!(app.recent_files.len(), 2);
    }

    fn test_crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    fn build_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for &(name, data) in entries {
            let offset = out.len() as u32;
            let crc = test_crc32(data);
            let size = data.len() as u32;
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u16;

            out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);

            central.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&name_len.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }
        let cd_offset = out.len() as u32;
        let cd_size = central.len() as u32;
        let count = entries.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn csv_opens_in_sheet_mode_searches_cells_and_toggles_to_editor() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("inventario.csv");
        fs::write(&csv, "Producto,Cantidad\nMonitor,3\nTeclado,12\n").unwrap();
        let (_, mut app) = fixture(dir.path());
        app.open_file(&csv).unwrap();
        assert!(app.is_sheet);
        assert!(app.sheet_delimited_text);
        assert!(app.preview);
        assert_eq!(app.sheet_view.document.as_ref().unwrap().sheets.len(), 1);

        app.handle(Command::FindAdvanced("Teclado".into(), true, false, false))
            .unwrap();
        assert!(app.status.contains("1 de 1"));
        assert_eq!(app.sheet_view.selected_cell, Some((2, 0)));

        app.handle(Command::Preview(false)).unwrap();
        assert!(!app.preview);
        assert!(app.workspace.buffer.is_some());
        app.handle(Command::Preview(true)).unwrap();
        assert!(app.preview);
    }

    #[test]
    fn docx_opens_in_readonly_rich_preview_and_ignores_editor_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let docx = dir.path().join("informe.docx");
        let doc_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Resumen</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>Contenido Word verificado</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;
        let zip_bytes = build_stored_zip(&[("word/document.xml", doc_xml)]);
        fs::write(&docx, zip_bytes).unwrap();

        let (_, mut app) = fixture(dir.path());
        app.open_file(&docx).unwrap();
        assert!(app.is_docx);
        assert!(app.is_markdown_active());
        assert!(app.preview);
        assert_eq!(app.doc_view.document.as_ref().unwrap().headings.len(), 1);

        app.handle(Command::Preview(false)).unwrap();
        assert!(app.preview);

        app.handle(Command::FindAdvanced(
            "verificado".into(),
            true,
            false,
            false,
        ))
        .unwrap();
        assert!(app.status.contains("1 de 1"));
    }

    #[test]
    fn pdf_opens_renders_zooms_and_searches_text() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("documento.pdf");
        fs::write(&pdf, crate::pdf_view::minimal_test_pdf()).unwrap();

        let (_, mut app) = fixture(dir.path());
        app.open_file(&pdf).unwrap();
        assert!(app.is_pdf);
        assert_eq!(app.pdf_view.total_pages, 1);
        assert!(app.pdf_view.cached_current_image().is_some());

        app.handle(Command::PdfZoom(1)).unwrap();
        assert_eq!(app.pdf_view.scale_percent(), 150);
        assert!(app.pdf_view.cached_current_image().is_some());

        app.handle(Command::FindAdvanced("Hola".into(), true, false, false))
            .unwrap();
        assert!(app.status.contains("Pág. 1 (1 de 1)"));
    }
}
