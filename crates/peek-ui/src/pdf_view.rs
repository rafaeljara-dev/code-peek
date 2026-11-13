use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::hayro_syntax::LoadPdfError;
use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::{
    interpret_page, Context as PdfContext, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, TransformExt,
};
use hayro::kurbo::{BezPath, Point, Rect};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

use crate::image_loader::RawRgbaImage;

const MAX_PDF_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RENDER_SIDE_PX: f32 = 3200.0;
/// Rendered pages are evicted by size, not by count: a zoomed page weighs far more.
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHED_THUMBS: usize = 96;
/// Font and glyph caches are shared across pages and rebuilt periodically to stay bounded.
const RENDERS_PER_CACHE: usize = 48;
pub const MIN_PDF_SCALE: f32 = 0.5;
pub const MAX_PDF_SCALE: f32 = 3.0;
pub const DEFAULT_PDF_SCALE: f32 = 1.35;
const MIN_FIT_SCALE: f32 = 0.25;
const MAX_FIT_SCALE: f32 = 4.0;
/// Logical width of a page thumbnail in the navigation strip.
pub const THUMB_WIDTH: f32 = 132.0;
const PAGE_MARGIN: f32 = 48.0;

pub fn is_pdf_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    page: usize,
    scale_pct: u32,
}

pub enum PdfJob {
    Document(u64, Arc<Pdf>),
    Close,
    Render {
        generation: u64,
        page: usize,
        scale: f32,
        scale_pct: u32,
        prerender: bool,
    },
    Thumbs {
        generation: u64,
        pages: Vec<usize>,
        scale_factor: f32,
    },
    Search {
        generation: u64,
        id: u64,
        query: String,
        case_sensitive: bool,
    },
}

pub enum PdfEvent {
    Page {
        generation: u64,
        page: usize,
        scale_pct: u32,
        result: std::result::Result<RawRgbaImage, String>,
    },
    Thumb {
        generation: u64,
        page: usize,
        image: RawRgbaImage,
    },
    Search {
        generation: u64,
        id: u64,
        pages: Vec<usize>,
        done: bool,
    },
}

/// One background thread per window renders pages, thumbnails and search text.
/// A newer page request replaces a pending one, so fast paging never queues stale work.
pub struct PdfWorker {
    jobs: Sender<PdfJob>,
}

impl PdfWorker {
    pub fn spawn(emit: impl Fn(PdfEvent) -> bool + Send + 'static) -> Self {
        let (jobs, rx) = mpsc::channel();
        std::thread::spawn(move || worker_loop(&rx, &emit));
        Self { jobs }
    }

    fn send(&self, job: PdfJob) {
        let _ = self.jobs.send(job);
    }
}

fn worker_loop(rx: &Receiver<PdfJob>, emit: &dyn Fn(PdfEvent) -> bool) {
    let mut next = None;
    loop {
        let job = match next.take() {
            Some(job) => job,
            None => match rx.recv() {
                Ok(job) => job,
                Err(_) => return,
            },
        };
        if let PdfJob::Document(generation, pdf) = job {
            match serve_document(generation, &pdf, rx, emit) {
                Some(job) => next = Some(job),
                None => return,
            }
        }
    }
}

struct SearchJob {
    id: u64,
    needle: String,
    case_sensitive: bool,
    next_page: usize,
    found: Vec<usize>,
}

/// Serves one document until another replaces it. `None` means the window is gone.
fn serve_document(
    generation: u64,
    pdf: &Pdf,
    rx: &Receiver<PdfJob>,
    emit: &dyn Fn(PdfEvent) -> bool,
) -> Option<PdfJob> {
    let total_pages = pdf.pages().len();
    let mut cache = RenderCache::new();
    let mut renders = 0usize;
    let mut texts: Vec<Option<String>> = vec![None; total_pages];
    let mut render = None;
    let mut prerender = None;
    let mut thumbs: VecDeque<usize> = VecDeque::new();
    let mut thumb_scale_factor = 1.0f32;
    let mut search: Option<SearchJob> = None;
    loop {
        let mut idle =
            render.is_none() && prerender.is_none() && thumbs.is_empty() && search.is_none();
        loop {
            let job = if idle {
                rx.recv().ok()?
            } else {
                match rx.try_recv() {
                    Ok(job) => job,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return None,
                }
            };
            idle = false;
            match job {
                PdfJob::Document(..) => return Some(job),
                PdfJob::Close => return Some(PdfJob::Close),
                PdfJob::Render {
                    generation: g,
                    page,
                    scale,
                    scale_pct,
                    prerender: low_priority,
                } if g == generation => {
                    if low_priority {
                        prerender = Some((page, scale, scale_pct));
                    } else {
                        render = Some((page, scale, scale_pct));
                    }
                }
                PdfJob::Thumbs {
                    generation: g,
                    pages,
                    scale_factor,
                } if g == generation => {
                    thumbs = pages.into();
                    thumb_scale_factor = scale_factor;
                }
                PdfJob::Search {
                    generation: g,
                    id,
                    query,
                    case_sensitive,
                } if g == generation => {
                    search = Some(SearchJob {
                        id,
                        needle: if case_sensitive {
                            query
                        } else {
                            query.to_lowercase()
                        },
                        case_sensitive,
                        next_page: 0,
                        found: Vec::new(),
                    });
                }
                _ => {}
            }
        }

        // One unit of work per turn keeps every request cancellable within a page.
        let visible = render.take();
        let page_job = visible.or_else(|| {
            if search.is_none() && thumbs.is_empty() {
                prerender.take()
            } else {
                None
            }
        });
        if let Some((page, scale, scale_pct)) = page_job {
            renders += 1;
            if renders.is_multiple_of(RENDERS_PER_CACHE) {
                cache = RenderCache::new();
            }
            let result = render_page(pdf, &cache, page, scale).map_err(|e| format!("{e:#}"));
            if !emit(PdfEvent::Page {
                generation,
                page,
                scale_pct,
                result,
            }) {
                return None;
            }
        } else if let Some(job) = search.as_mut() {
            let page = job.next_page;
            let text = texts[page].get_or_insert_with(|| extract_pdf_page_text(pdf, page));
            let matched = if job.case_sensitive {
                text.contains(&job.needle)
            } else {
                text.to_lowercase().contains(&job.needle)
            };
            if matched {
                job.found.push(page);
            }
            job.next_page += 1;
            let done = job.next_page >= total_pages;
            if done || (matched && job.found.len() == 1) || job.next_page % 32 == 0 {
                let event = PdfEvent::Search {
                    generation,
                    id: job.id,
                    pages: job.found.clone(),
                    done,
                };
                if !emit(event) {
                    return None;
                }
            }
            if done {
                search = None;
            }
        } else if let Some(page) = thumbs.pop_front() {
            let Some((width, _)) = page_size(pdf, page) else {
                continue;
            };
            let scale = THUMB_WIDTH * thumb_scale_factor / width.max(1.0);
            if let Ok(image) = render_page(pdf, &cache, page, scale) {
                if !emit(PdfEvent::Thumb {
                    generation,
                    page,
                    image,
                }) {
                    return None;
                }
            }
        }
    }
}

pub struct PdfViewState {
    pub document: Option<Arc<Pdf>>,
    pub total_pages: usize,
    pub current_page: usize,
    /// Manual zoom: logical pixels per PDF point. Ignored while `fit_width` holds.
    pub scale: f32,
    pub fit_width: bool,
    /// Logical pixels available to the page, reported by the window.
    pub viewport_width: f32,
    /// Physical pixels per logical pixel; pages are rasterized for the real screen.
    pub pixel_ratio: f32,
    pub loading: bool,
    pub dirty: bool,
    pub find_query: String,
    find_case: bool,
    pub find_pages: Vec<usize>,
    pub find_cursor: usize,
    pub searching: bool,
    search_id: u64,
    search_jumped: bool,
    cache: VecDeque<(CacheKey, RawRgbaImage)>,
    text_cache: Vec<Option<String>>,
    thumbs: HashMap<usize, RawRgbaImage>,
    thumb_order: VecDeque<usize>,
    thumb_request: Vec<usize>,
    pub generation: u64,
    worker: Option<PdfWorker>,
}

impl Default for PdfViewState {
    fn default() -> Self {
        Self {
            document: None,
            total_pages: 0,
            current_page: 0,
            scale: DEFAULT_PDF_SCALE,
            fit_width: true,
            viewport_width: 0.0,
            pixel_ratio: 1.0,
            loading: false,
            dirty: false,
            find_query: String::new(),
            find_case: false,
            find_pages: Vec::new(),
            find_cursor: 0,
            searching: false,
            search_id: 0,
            search_jumped: false,
            cache: VecDeque::new(),
            text_cache: Vec::new(),
            thumbs: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_request: Vec::new(),
            generation: 0,
            worker: None,
        }
    }
}

impl PdfViewState {
    pub fn set_worker(&mut self, worker: PdfWorker) {
        self.worker = Some(worker);
    }

    fn reset_document_state(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.current_page = 0;
        self.loading = false;
        self.find_query.clear();
        self.find_pages.clear();
        self.find_cursor = 0;
        self.searching = false;
        self.cache.clear();
        self.thumbs.clear();
        self.thumb_order.clear();
        self.thumb_request.clear();
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        if self.document.take().is_some() {
            if let Some(worker) = &self.worker {
                worker.send(PdfJob::Close);
            }
        }
        self.reset_document_state();
        self.total_pages = 0;
        self.text_cache.clear();
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        let meta =
            fs::metadata(path).with_context(|| format!("No se pudo leer {}", path.display()))?;
        if meta.len() > MAX_PDF_FILE_BYTES {
            bail!(
                "El archivo PDF supera el límite seguro de {} MB",
                MAX_PDF_FILE_BYTES / (1024 * 1024)
            );
        }
        let mut file =
            File::open(path).with_context(|| format!("No se pudo abrir {}", path.display()))?;
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        file.by_ref()
            .take(MAX_PDF_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PDF_FILE_BYTES {
            bail!("El archivo PDF excede el tamaño máximo permitido");
        }
        self.open_bytes(bytes)
    }

    pub fn open_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        let pdf = match Pdf::new(Arc::new(bytes)) {
            Ok(pdf) => pdf,
            Err(LoadPdfError::Decryption(_)) => {
                bail!("El PDF está cifrado con contraseña y no se puede abrir todavía");
            }
            Err(LoadPdfError::Invalid) => {
                bail!("El archivo PDF es inválido o está dañado");
            }
        };
        let total_pages = pdf.pages().len();
        if total_pages == 0 {
            bail!("El documento PDF no contiene páginas");
        }
        let pdf = Arc::new(pdf);
        self.reset_document_state();
        if let Some(worker) = &self.worker {
            worker.send(PdfJob::Document(self.generation, pdf.clone()));
        }
        self.document = Some(pdf);
        self.total_pages = total_pages;
        self.text_cache = vec![None; total_pages];
        Ok(())
    }

    fn current_page_size(&self) -> (f32, f32) {
        self.document
            .as_ref()
            .and_then(|pdf| page_size(pdf, self.current_page))
            .unwrap_or((612.0, 792.0))
    }

    /// Logical pixels per PDF point for the page on screen.
    pub fn effective_scale(&self) -> f32 {
        if self.fit_width && self.viewport_width > PAGE_MARGIN + 80.0 {
            ((self.viewport_width - PAGE_MARGIN) / self.current_page_size().0.max(1.0))
                .clamp(MIN_FIT_SCALE, MAX_FIT_SCALE)
        } else {
            self.scale
        }
    }

    pub fn scale_percent(&self) -> u16 {
        (self.effective_scale() * 100.0).round() as u16
    }

    /// Logical size at which the page is laid out, independent of the bitmap resolution.
    pub fn display_size(&self) -> (f32, f32) {
        let (width, height) = self.current_page_size();
        let scale = self.effective_scale();
        (width * scale, height * scale)
    }

    fn render_scale(&self) -> (f32, u32) {
        let scale = self.effective_scale() * self.pixel_ratio.clamp(0.5, 4.0);
        (scale, (scale * 100.0).round() as u32)
    }

    fn cached(&self, page: usize, scale_pct: u32) -> Option<&RawRgbaImage> {
        let key = CacheKey { page, scale_pct };
        self.cache
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, img)| img)
    }

    /// The bitmap at the exact resolution for the current page, if it is ready.
    pub fn cached_current_image(&self) -> Option<&RawRgbaImage> {
        self.cached(self.current_page, self.render_scale().1)
    }

    /// The best bitmap to show now: exact, or the same page at another zoom while it re-renders.
    pub fn display_image(&self) -> Option<&RawRgbaImage> {
        self.cached_current_image().or_else(|| {
            self.cache
                .iter()
                .rev()
                .find(|(k, _)| k.page == self.current_page)
                .map(|(_, img)| img)
        })
    }

    pub fn insert_cached_page(&mut self, page: usize, scale_pct: u32, image: RawRgbaImage) {
        let key = CacheKey { page, scale_pct };
        self.cache.retain(|(k, _)| *k != key);
        self.cache.push_back((key, image));
        let bytes = |cache: &VecDeque<(CacheKey, RawRgbaImage)>| -> usize {
            cache.iter().map(|(_, img)| image_bytes(img)).sum()
        };
        while self.cache.len() > 1 && bytes(&self.cache) > MAX_CACHE_BYTES {
            self.cache.pop_front();
        }
        if page == self.current_page && scale_pct == self.render_scale().1 {
            self.loading = false;
            self.dirty = true;
        }
    }

    /// Renders the visible page (and warms the next one) unless it is already cached.
    pub fn request_render(&mut self) {
        let Some(pdf) = self.document.clone() else {
            return;
        };
        let page = self.current_page;
        let (scale, scale_pct) = self.render_scale();
        self.dirty = true;
        if self.cached(page, scale_pct).is_none() {
            match &self.worker {
                Some(worker) => {
                    self.loading = true;
                    worker.send(PdfJob::Render {
                        generation: self.generation,
                        page,
                        scale,
                        scale_pct,
                        prerender: false,
                    });
                }
                None => {
                    let cache = RenderCache::new();
                    if let Ok(image) = render_page(&pdf, &cache, page, scale) {
                        self.insert_cached_page(page, scale_pct, image);
                    }
                }
            }
        } else {
            self.loading = false;
        }
        let next = page + 1;
        if next < self.total_pages && self.cached(next, scale_pct).is_none() {
            if let Some(worker) = &self.worker {
                worker.send(PdfJob::Render {
                    generation: self.generation,
                    page: next,
                    scale,
                    scale_pct,
                    prerender: true,
                });
            }
        }
    }

    /// Viewport size or screen density changed. Returns true when the page must be re-rendered.
    pub fn set_viewport(&mut self, width: f32, pixel_ratio: f32) -> bool {
        let before = (self.render_scale().1, self.display_size());
        self.viewport_width = width.max(0.0);
        self.pixel_ratio = if pixel_ratio.is_finite() && pixel_ratio > 0.0 {
            pixel_ratio
        } else {
            1.0
        };
        let after = (self.render_scale().1, self.display_size());
        if before != after {
            self.dirty = true;
        }
        self.document.is_some() && before.0 != after.0
    }

    pub fn step_page(&mut self, delta: isize) -> bool {
        if self.total_pages == 0 {
            return false;
        }
        let target = if delta < 0 {
            self.current_page.saturating_sub(delta.unsigned_abs())
        } else {
            (self.current_page + delta as usize).min(self.total_pages - 1)
        };
        self.set_page(target)
    }

    pub fn set_page(&mut self, target: usize) -> bool {
        if target < self.total_pages && target != self.current_page {
            self.current_page = target;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn go_page(&mut self, one_based: usize) -> Result<bool> {
        if self.total_pages == 0 {
            bail!("No hay ningún PDF abierto");
        }
        if one_based == 0 || one_based > self.total_pages {
            bail!("La página debe estar entre 1 y {}", self.total_pages);
        }
        Ok(self.set_page(one_based - 1))
    }

    /// `delta == 0` returns to fit-width; otherwise zooms manually from the current scale.
    pub fn adjust_zoom(&mut self, delta: i32) -> bool {
        let before = self.scale_percent();
        if delta == 0 {
            self.fit_width = true;
        } else {
            let step = if delta > 0 { 0.15 } else { -0.15 };
            self.scale = (self.effective_scale() + step).clamp(MIN_PDF_SCALE, MAX_PDF_SCALE);
            self.fit_width = false;
        }
        self.dirty = true;
        self.scale_percent() != before
    }

    pub fn current_page_text(&mut self) -> String {
        let page = self.current_page;
        self.ensure_page_text(page)
    }

    fn cycle(&mut self, forward: bool) {
        if !self.find_pages.is_empty() {
            self.find_cursor = if forward {
                (self.find_cursor + 1) % self.find_pages.len()
            } else if self.find_cursor == 0 {
                self.find_pages.len() - 1
            } else {
                self.find_cursor - 1
            };
        }
    }

    fn find_summary(&mut self) -> String {
        if let Some(&page_idx) = self.find_pages.get(self.find_cursor) {
            self.current_page = page_idx;
            self.dirty = true;
            format!(
                "Pág. {} ({} de {}{})",
                page_idx + 1,
                self.find_cursor + 1,
                self.find_pages.len(),
                if self.searching { "…" } else { "" }
            )
        } else if self.searching {
            "Buscando…".into()
        } else {
            "Sin coincidencias".into()
        }
    }

    /// Pages are matched in the background; repeating the query steps through the results.
    pub fn find(&mut self, query: &str, forward: bool, case_sensitive: bool) -> String {
        if self.document.is_none() || self.total_pages == 0 {
            return "Sin PDF".into();
        }
        if query.is_empty() {
            self.find_query.clear();
            self.find_pages.clear();
            self.find_cursor = 0;
            self.searching = false;
            return String::new();
        }
        let same = self.find_query == query && self.find_case == case_sensitive;
        if same && (self.searching || !self.find_pages.is_empty()) {
            self.cycle(forward);
            return self.find_summary();
        }
        self.find_query = query.to_owned();
        self.find_case = case_sensitive;
        self.find_pages.clear();
        self.find_cursor = 0;
        self.search_id = self.search_id.wrapping_add(1);
        self.search_jumped = false;
        if let Some(worker) = &self.worker {
            self.searching = true;
            worker.send(PdfJob::Search {
                generation: self.generation,
                id: self.search_id,
                query: query.to_owned(),
                case_sensitive,
            });
            return self.find_summary();
        }
        let needle = if case_sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        for page_idx in 0..self.total_pages {
            let text = self.ensure_page_text(page_idx);
            let matched = if case_sensitive {
                text.contains(&needle)
            } else {
                text.to_lowercase().contains(&needle)
            };
            if matched {
                self.find_pages.push(page_idx);
            }
        }
        self.find_summary()
    }

    /// Applies background search progress. Returns the summary when it belongs to the live query.
    pub fn apply_search(&mut self, id: u64, pages: Vec<usize>, done: bool) -> Option<String> {
        if id != self.search_id || !self.searching {
            return None;
        }
        self.find_pages = pages;
        self.searching = !done;
        if self.search_jumped || self.find_pages.is_empty() {
            // Keep the page the user is reading; only the counter changes.
            let position = self.find_pages.get(self.find_cursor).copied();
            return Some(match position {
                Some(page) => format!(
                    "Pág. {} ({} de {}{})",
                    page + 1,
                    self.find_cursor + 1,
                    self.find_pages.len(),
                    if self.searching { "…" } else { "" }
                ),
                None if self.searching => "Buscando…".into(),
                None => "Sin coincidencias".into(),
            });
        }
        self.search_jumped = true;
        self.find_cursor = 0;
        Some(self.find_summary())
    }

    fn ensure_page_text(&mut self, page_index: usize) -> String {
        if let Some(Some(cached)) = self.text_cache.get(page_index) {
            return cached.clone();
        }
        let extracted = self
            .document
            .as_ref()
            .map(|pdf| extract_pdf_page_text(pdf, page_index))
            .unwrap_or_default();
        if let Some(slot) = self.text_cache.get_mut(page_index) {
            *slot = Some(extracted.clone());
        }
        extracted
    }

    /// Height of a thumbnail row for a uniform, virtualized strip.
    pub fn thumb_height(&self) -> f32 {
        let (width, height) = self
            .document
            .as_ref()
            .and_then(|pdf| page_size(pdf, 0))
            .unwrap_or((612.0, 792.0));
        (THUMB_WIDTH * height / width.max(1.0)).clamp(60.0, 260.0)
    }

    pub fn thumb(&self, page: usize) -> Option<&RawRgbaImage> {
        self.thumbs.get(&page)
    }

    /// Asks for the thumbnails around the visible rows of the strip.
    pub fn request_thumbs(&mut self, first: usize, count: usize) {
        if self.document.is_none() {
            return;
        }
        let start = first.saturating_sub(2);
        let end = (first + count + 4).min(self.total_pages);
        let wanted: Vec<usize> = (start..end)
            .filter(|page| !self.thumbs.contains_key(page))
            .collect();
        if wanted.is_empty() || wanted == self.thumb_request {
            return;
        }
        self.thumb_request = wanted.clone();
        if let Some(worker) = &self.worker {
            worker.send(PdfJob::Thumbs {
                generation: self.generation,
                pages: wanted,
                scale_factor: self.pixel_ratio.clamp(0.5, 4.0),
            });
        }
    }

    /// Stores a thumbnail and returns the page evicted to keep the strip bounded.
    pub fn insert_thumb(&mut self, page: usize, image: RawRgbaImage) -> Option<usize> {
        if self.thumbs.insert(page, image).is_none() {
            self.thumb_order.push_back(page);
        }
        if self.thumb_order.len() > MAX_CACHED_THUMBS {
            let evicted = self.thumb_order.pop_front()?;
            self.thumbs.remove(&evicted);
            return Some(evicted);
        }
        None
    }
}

fn image_bytes(image: &RawRgbaImage) -> usize {
    image.width() as usize * image.height() as usize * 4
}

fn page_size(pdf: &Pdf, page_index: usize) -> Option<(f32, f32)> {
    let (width, height) = pdf.pages().get(page_index)?.render_dimensions();
    (width > 0.0 && height > 0.0).then_some((width, height))
}

fn render_page<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    page_index: usize,
    requested_scale: f32,
) -> Result<RawRgbaImage> {
    let pages = pdf.pages();
    let page = pages
        .get(page_index)
        .with_context(|| format!("La página {} no existe en el PDF", page_index + 1))?;
    let (raw_w, raw_h) = page.render_dimensions();
    if raw_w <= 0.0 || raw_h <= 0.0 {
        bail!("La página PDF tiene dimensiones vacías");
    }
    let max_dim_scale = MAX_RENDER_SIDE_PX / raw_w.max(raw_h);
    let effective_scale = requested_scale.min(max_dim_scale).max(0.02);

    let interpreter_settings = InterpreterSettings::default();
    let render_settings = RenderSettings::default();
    let pixmap_settings = PixmapSettings {
        x_scale: effective_scale,
        y_scale: effective_scale,
        bg_color: WHITE,
    };
    let pixmap = hayro::render(
        page,
        cache,
        &interpreter_settings,
        &render_settings,
        &pixmap_settings,
    );
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    if width == 0 || height == 0 {
        bail!("La página PDF renderizó vacía");
    }
    // WHITE background keeps alpha == 255 across the canvas, so RGBA bytes are directly opaque.
    Ok(RawRgbaImage::from_slice(
        pixmap.data_as_u8_slice(),
        width,
        height,
    ))
}

#[cfg(test)]
pub fn render_pdf_page(pdf: &Pdf, page_index: usize, scale: f32) -> Result<RawRgbaImage> {
    render_page(pdf, &RenderCache::new(), page_index, scale)
}

pub fn extract_pdf_page_text(pdf: &Pdf, page_index: usize) -> String {
    let Some(page) = pdf.pages().get(page_index) else {
        return String::new();
    };
    let (w, h) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut context = PdfContext::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, f64::from(w.max(1.0)), f64::from(h.max(1.0))),
        &cache,
        pdf.xref(),
        InterpreterSettings::default(),
    );
    let mut device = TextCollector::default();
    interpret_page(page, &mut context, &mut device);
    device.out
}

#[derive(Default)]
struct TextCollector {
    out: String,
    last_end_x: f64,
    last_y: f64,
    has_prev: bool,
}

impl Device<'_> for TextCollector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'_>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(
        &mut self,
        _: f32,
        _: Option<SoftMask<'_>>,
        _: hayro::hayro_interpret::BlendMode,
    ) {
    }
    fn draw_image(&mut self, _: Image<'_, '_>, _: ImageDrawProps<'_>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, glyph_run: &GlyphRun<'_, '_>, _: DrawProps<'_>, _: &DrawMode) {
        for glyph in glyph_run.glyphs() {
            let Some(unicode) = glyph.as_unicode() else {
                continue;
            };
            let transform = glyph.transform();
            let coeffs = transform.as_coeffs();
            let x = coeffs[4];
            let y = coeffs[5];
            let em = (coeffs[0].hypot(coeffs[1]) * 1000.0).max(6.0);
            if self.has_prev {
                if (y - self.last_y).abs() > (em * 0.45).max(4.0) {
                    if !self.out.ends_with('\n') {
                        self.out.push('\n');
                    }
                } else if (x - self.last_end_x) > (em * 0.20).max(1.5)
                    && !self.out.ends_with([' ', '\n'])
                {
                    self.out.push(' ');
                }
            }
            match unicode {
                BfString::Char(ch) => self.out.push(ch),
                BfString::String(s) => self.out.push_str(&s),
            }
            let adv = match &**glyph {
                Glyph::Outline(outline) => outline.advance_width(),
                Glyph::Type3(t3) => t3.advance_width(),
            };
            self.last_end_x = if let Some(advance) = adv {
                (transform * Point::new(f64::from(advance), 0.0)).x
            } else {
                x + em * 0.5
            };
            self.last_y = y;
            self.has_prev = true;
        }
    }
}

/// Builds a PDF whose pages each show one line of WinAnsi text (Latin-1 bytes).
#[cfg(test)]
pub(crate) fn test_pdf(pages: &[&str]) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes());
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    for (i, text) in pages.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                5 + i * 2
            )
            .into_bytes(),
        );
        let mut stream = b"BT /F1 18 Tf 24 120 Td (".to_vec();
        stream.extend(text.chars().map(|ch| ch as u8));
        stream.extend_from_slice(b") Tj ET");
        let mut object = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        object.extend_from_slice(&stream);
        object.extend_from_slice(b"\nendstream");
        objects.push(object);
    }
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[cfg(test)]
pub(crate) fn minimal_test_pdf() -> Vec<u8> {
    test_pdf(&["Hola PDF"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn renders_minimal_pdf_page_and_extracts_text() {
        let mut state = PdfViewState::default();
        state.open_bytes(minimal_test_pdf()).unwrap();
        assert_eq!(state.total_pages, 1);

        let pdf = state.document.as_ref().unwrap();
        let img = render_pdf_page(pdf, 0, 1.0).unwrap();
        assert_eq!(img.width(), 300);
        assert_eq!(img.height(), 200);

        let text = state.current_page_text();
        assert!(text.contains("Hola PDF"), "extracted: {text:?}");
        assert_eq!(state.find("Hola", true, false), "Pág. 1 (1 de 1)");
    }

    #[test]
    fn extracts_spanish_accents_without_replacement_characters() {
        let mut state = PdfViewState::default();
        state
            .open_bytes(test_pdf(&["Año niño canción ¿Qué?"]))
            .unwrap();
        let text = state.current_page_text();
        assert!(
            text.contains("Año niño canción ¿Qué?"),
            "extracted: {text:?}"
        );
        assert!(!text.contains('\u{FFFD}'));
        assert_eq!(state.find("canción", true, false), "Pág. 1 (1 de 1)");
    }

    #[test]
    fn page_cache_is_bounded_by_bytes_and_keeps_the_newest_page() {
        let mut state = PdfViewState::default();
        // 2048 x 2048 RGBA = 16 MiB per page: only four fit in the 64 MiB budget.
        for page in 0..8 {
            let image = RawRgbaImage::new(vec![255; 2048 * 2048 * 4], 2048, 2048);
            state.insert_cached_page(page, 100, image);
        }
        let bytes: usize = state.cache.iter().map(|(_, img)| image_bytes(img)).sum();
        assert!(bytes <= MAX_CACHE_BYTES);
        assert_eq!(state.cache.len(), 4);
        assert!(state.cached(7, 100).is_some());
        assert!(state.cached(0, 100).is_none());
    }

    #[test]
    fn fits_width_then_switches_to_manual_zoom_and_renders_for_screen_density() {
        let mut state = PdfViewState::default();
        state.open_bytes(minimal_test_pdf()).unwrap();
        assert_eq!(state.scale_percent(), 135);
        // 648 logical px minus the 48 px margin over a 300 pt page = 200 %.
        assert!(state.set_viewport(648.0, 1.5));
        assert_eq!(state.scale_percent(), 200);
        assert_eq!(state.display_size(), (600.0, 400.0));
        state.request_render();
        let image = state.cached_current_image().unwrap();
        assert_eq!((image.width(), image.height()), (900, 600));

        assert!(state.adjust_zoom(-1));
        assert!(!state.fit_width);
        assert_eq!(state.scale_percent(), 185);
        assert!(state.adjust_zoom(0));
        assert_eq!(state.scale_percent(), 200);
    }

    #[test]
    fn worker_renders_latest_page_thumbnails_and_searches_in_background() {
        let (tx, rx) = mpsc::channel();
        let mut state = PdfViewState::default();
        state.set_worker(PdfWorker::spawn(move |event| tx.send(event).is_ok()));
        state
            .open_bytes(test_pdf(&["Uno", "Dos señal", "Tres", "Cuatro señal"]))
            .unwrap();
        state.set_page(3);
        state.request_render();
        state.request_thumbs(0, 2);
        assert_eq!(state.find("SEÑAL", true, false), "Buscando…");

        let mut summary = String::new();
        while state.loading || state.searching || state.thumb(0).is_none() {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                PdfEvent::Page {
                    generation,
                    page,
                    scale_pct,
                    result,
                } => {
                    assert_eq!(generation, state.generation);
                    state.insert_cached_page(page, scale_pct, result.unwrap());
                }
                PdfEvent::Thumb { page, image, .. } => {
                    state.insert_thumb(page, image);
                }
                PdfEvent::Search {
                    id, pages, done, ..
                } => {
                    if let Some(text) = state.apply_search(id, pages, done) {
                        summary = text;
                        state.request_render();
                    }
                }
            }
        }
        assert!(state.cached_current_image().is_some());
        assert_eq!(state.thumb(0).unwrap().width(), 132);
        assert_eq!(state.find_pages, vec![1, 3]);
        assert_eq!(summary, "Pág. 2 (1 de 2)");
        assert_eq!(state.current_page, 1);
        assert_eq!(state.find("SEÑAL", true, false), "Pág. 4 (2 de 2)");
    }
}
