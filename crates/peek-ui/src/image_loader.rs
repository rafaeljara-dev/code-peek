use std::collections::HashMap;
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use image::ImageDecoder;

/// Decoded pixel buffer that is safely Send + Sync across worker and UI threads.
#[derive(Clone)]
pub struct RawRgbaImage {
    pub data: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
    pub width: u32,
    pub height: u32,
}

impl RawRgbaImage {
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            data: slint::SharedPixelBuffer::clone_from_slice(&data, width, height),
            width,
            height,
        }
    }

    /// Converts raw RGBA pixels into a Slint Image on the UI thread.
    pub fn to_slint_image(&self) -> slint::Image {
        slint::Image::from_rgba8(self.data.clone())
    }
}

const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SVG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_EMBEDDED_BYTES: u64 = 32 * 1024 * 1024;

fn open_raster(path: &Path) -> Result<image::ImageReader<std::io::BufReader<std::fs::File>>> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_INPUT_BYTES {
        bail!("La imagen no es un archivo regular o supera 128 MiB de entrada");
    }
    let mut reader = std::io::BufReader::new(file);
    if reader.fill_buf()?.starts_with(&[0, 0, 1, 0]) {
        validate_ico(&mut reader, metadata.len())?;
    }
    let mut reader = image::ImageReader::new(reader);
    if let Ok(format) = image::ImageFormat::from_path(path) {
        reader.set_format(format);
    }
    Ok(reader.with_guessed_format()?)
}

/// Validate directory entries before any decoder can trust their advertised lengths.
fn validate_ico(reader: &mut (impl Read + Seek), length: u64) -> Result<()> {
    let mut header = [0u8; 6];
    reader.read_exact(&mut header)?;
    let count = u16::from_le_bytes([header[4], header[5]]) as u64;
    let directory_end = 6 + count * 16;
    if count == 0 || count > 256 || directory_end > length {
        bail!("Directorio ICO vacío, truncado o demasiado grande");
    }
    for _ in 0..count {
        let mut entry = [0u8; 16];
        reader.read_exact(&mut entry)?;
        let bytes = u64::from(u32::from_le_bytes(entry[8..12].try_into()?));
        let offset = u64::from(u32::from_le_bytes(entry[12..16].try_into()?));
        if bytes == 0
            || bytes > MAX_INPUT_BYTES
            || offset < directory_end
            || offset + bytes > length
        {
            bail!("La entrada ICO apunta fuera del archivo o supera el presupuesto");
        }
    }
    reader.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("El SVG supera el límite de lectura o descompresión");
    }
    Ok(bytes)
}

fn read_svg(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        bail!("El SVG no es un archivo regular o supera el presupuesto de entrada");
    }
    let data = read_bounded(file, limit)?;
    if data.starts_with(&[0x1f, 0x8b]) {
        read_bounded(flate2::read::MultiGzDecoder::new(data.as_slice()), limit)
    } else {
        Ok(data)
    }
}

fn safe_svg_tree(path: &Path) -> Result<usvg::Tree> {
    let data = read_svg(path, MAX_SVG_BYTES)?;
    let text = std::str::from_utf8(&data).context("El SVG no es UTF-8 válido")?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 50_000,
            ..Default::default()
        },
    )?;
    let mut references = 0;
    for node in doc.descendants().filter(|node| node.is_element()) {
        if node.ancestors().take(66).count() > 64 || node.attributes().len() > 128 {
            bail!("El SVG supera el límite de anidamiento o atributos");
        }
        if node.has_tag_name("use") {
            references += 1;
            if references > 512 {
                bail!("El SVG contiene demasiadas referencias");
            }
            let href = node
                .attribute("href")
                .or_else(|| node.attribute(("http://www.w3.org/1999/xlink", "href")));
            if let Some(id) = href.and_then(|href| href.strip_prefix('#')) {
                if doc
                    .descendants()
                    .find(|n| n.attribute("id") == Some(id))
                    .is_some_and(|n| n.descendants().any(|child| child.has_tag_name("use")))
                {
                    bail!("El SVG contiene referencias use anidadas");
                }
            }
        }
    }
    let budget = AtomicU64::new(MAX_EMBEDDED_BYTES);
    let default_data = usvg::ImageHrefResolver::default_data_resolver();
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            // A document never grants filesystem/UNC/network authority to an image href.
            resolve_string: Box::new(|_, _| None),
            resolve_data: Box::new(move |mime, data, options| {
                if !matches!(
                    mime,
                    "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp"
                ) {
                    return None; // No nested SVG or recursively compressed data documents.
                }
                let mut reader = image::ImageReader::new(std::io::Cursor::new(data.as_slice()))
                    .with_guessed_format()
                    .ok()?;
                let mut limits = image::Limits::default();
                limits.max_alloc = Some(MAX_EMBEDDED_BYTES);
                limits.max_image_width = Some(MAX_DIMENSION);
                limits.max_image_height = Some(MAX_DIMENSION);
                reader.limits(limits);
                let decoder = reader.into_decoder().ok()?;
                let (w, h) = decoder.dimensions();
                validate_dimensions(w, h, decoder.total_bytes()).ok()?;
                let cost = decoder.total_bytes().max(u64::from(w) * u64::from(h) * 4);
                budget
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        remaining.checked_sub(cost)
                    })
                    .ok()?;
                drop(decoder);
                default_data(mime, data, options)
            }),
        },
        ..Default::default()
    };
    Ok(usvg::Tree::from_xmltree(&doc, &options)?)
}

fn validate_dimensions(width: u32, height: u32, bytes: u64) -> Result<()> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) * 4 > MAX_DECODE_BYTES
        || bytes > MAX_DECODE_BYTES
    {
        bail!("La imagen supera el límite de 128 MiB de píxeles o 16.384 px por dimensión");
    }
    Ok(())
}

fn decode_raster(
    reader: image::ImageReader<std::io::BufReader<std::fs::File>>,
) -> Result<image::DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    let mut reader = reader;
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    // Check area and decoder output before DynamicImage allocates the complete buffer.
    validate_dimensions(width, height, decoder.total_bytes())?;
    Ok(image::DynamicImage::from_decoder(decoder)?)
}

#[derive(Clone)]
pub struct ImageDetails {
    pub raw: RawRgbaImage,
    pub width: u32,
    pub height: u32,
    pub format_name: &'static str,
    pub file_size_bytes: u64,
    pub summary: String,
}

/// Returns true if the file path or header matches a known image format.
pub fn is_image_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "jpe" | "jfif" | "webp" | "gif" | "bmp" | "ico" | "tiff"
        | "tif" | "svg" | "svgz" | "avif" | "qoi" | "tga" => true,
        _ => {
            if let Ok(mut file) = std::fs::File::open(path) {
                use std::io::Read;
                let mut magic = [0u8; 16];
                if let Ok(n) = file.read(&mut magic) {
                    if n >= 4 {
                        return is_magic_image(&magic[..n]);
                    }
                }
            }
            false
        }
    }
}

fn is_magic_image(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return true;
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return true;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return true;
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return true;
    }
    if bytes.starts_with(b"BM") {
        return true;
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return true;
    }
    if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        return true;
    }
    if bytes.starts_with(b"qoif") {
        return true;
    }
    let s = String::from_utf8_lossy(bytes);
    if s.contains("<svg") || s.contains("<?xml") {
        return true;
    }
    false
}

/// Formats byte count into human-readable size string.
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Loads an SVG image rendered sharply with high quality into raw RGBA pixels.
pub fn load_svg_raw(path: &Path) -> Result<(RawRgbaImage, u32, u32)> {
    let tree = safe_svg_tree(path)
        .context("No se pudo interpretar el archivo SVG como gráfico vectorial")?;

    let size = tree.size();
    let native_w = size.width().round() as u32;
    let native_h = size.height().round() as u32;

    let scale = if native_w < 512 && native_h < 512 {
        512.0 / size.width().max(size.height())
    } else {
        (4096.0 / size.width().max(size.height())).min(1.0)
    };
    let target_w = (size.width() * scale).round().max(1.0) as u32;
    let target_h = (size.height() * scale).round().max(1.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h)
        .context("No se pudo crear el buffer para el SVG")?;

    let sx = target_w as f32 / size.width();
    let sy = target_h as f32 / size.height();
    let transform = tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let raw = RawRgbaImage::new(pixmap.take(), target_w, target_h);

    Ok((raw, native_w.max(target_w), native_h.max(target_h)))
}

/// Loads a small thumbnail of an SVG as raw RGBA pixels.
pub fn load_svg_thumb_raw(path: &Path, max_dim: u32) -> Result<RawRgbaImage> {
    let tree = safe_svg_tree(path)?;
    let size = tree.size();
    let aspect = size.width() / size.height();
    let (w, h) = if aspect >= 1.0 {
        (max_dim, (max_dim as f32 / aspect).max(1.0).round() as u32)
    } else {
        (((max_dim as f32 * aspect).max(1.0)).round() as u32, max_dim)
    };
    let w = w.max(1);
    let h = h.max(1);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("Pixmap alloc failed")?;
    let transform =
        tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(RawRgbaImage::new(pixmap.take(), w, h))
}

/// Loads raster images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF, QOI, TGA, etc.) into raw RGBA pixels.
pub fn load_raster_image_raw(path: &Path) -> Result<(RawRgbaImage, u32, u32, &'static str)> {
    let reader = open_raster(path)
        .with_context(|| format!("No se pudo abrir el archivo: {}", path.display()))?;

    let format = reader.format();
    let format_name = match format {
        Some(image::ImageFormat::Png) => "PNG",
        Some(image::ImageFormat::Jpeg) => "JPEG",
        Some(image::ImageFormat::WebP) => "WebP",
        Some(image::ImageFormat::Gif) => "GIF",
        Some(image::ImageFormat::Bmp) => "BMP",
        Some(image::ImageFormat::Ico) => "ICO",
        Some(image::ImageFormat::Tiff) => "TIFF",
        Some(image::ImageFormat::Qoi) => "QOI",
        Some(image::ImageFormat::Tga) => "TGA",
        Some(image::ImageFormat::Avif) => "AVIF",
        _ => "Imagen",
    };

    let decoded = decode_raster(reader)
        .with_context(|| format!("No se pudo decodificar la imagen: {}", path.display()))?;

    let width = decoded.width();
    let height = decoded.height();
    let rgba = decoded.into_rgba8();

    let raw = RawRgbaImage::new(rgba.into_raw(), width, height);

    Ok((raw, width, height, format_name))
}

/// Loads a small thumbnail of a raster image as raw RGBA pixels.
pub fn load_raster_thumb_raw(path: &Path, max_dim: u32) -> Result<RawRgbaImage> {
    let reader = open_raster(path)?;
    let decoded = decode_raster(reader)?;
    let thumb = decoded.thumbnail(max_dim, max_dim);
    let (width, height) = (thumb.width(), thumb.height());
    let rgba = thumb.into_rgba8();
    Ok(RawRgbaImage::new(rgba.into_raw(), width, height))
}

/// Fully loads an image and computes its presentation metadata.
pub fn load_image_full(path: &Path) -> Result<ImageDetails> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("No se pudo obtener metadatos de: {}", path.display()))?;
    let file_size_bytes = metadata.len();
    let size_str = format_file_size(file_size_bytes);

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let (raw, width, height, format_name) = if ext == "svg" || ext == "svgz" {
        let (raw, w, h) = load_svg_raw(path)?;
        (raw, w, h, "SVG")
    } else {
        load_raster_image_raw(path)?
    };

    let summary = format!("{format_name} · {width} × {height} px · {size_str}");

    Ok(ImageDetails {
        raw,
        width,
        height,
        format_name,
        file_size_bytes,
        summary,
    })
}

/// Loads a thumbnail image of the given file path as raw pixels.
pub fn load_image_thumbnail_raw(path: &Path, max_dimension: u32) -> Option<RawRgbaImage> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "svg" || ext == "svgz" {
        load_svg_thumb_raw(path, max_dimension).ok()
    } else {
        load_raster_thumb_raw(path, max_dimension).ok()
    }
}

struct CachedThumbnail {
    stamp: (SystemTime, u64),
    image: Option<RawRgbaImage>,
    used: u64,
}

#[derive(Default)]
struct ThumbnailStore {
    entries: HashMap<(PathBuf, u32), CachedThumbnail>,
    bytes: usize,
    clock: u64,
}

type ThumbnailMap = Arc<Mutex<ThumbnailStore>>;

/// Thread-safe in-memory cache for file tree thumbnails (Send + Sync).
#[derive(Clone, Default)]
pub struct ThumbnailCache {
    cache: ThumbnailMap,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(&self, path: &Path) -> Option<RawRgbaImage> {
        self.get_or_create_sized(path, 36)
    }

    /// Disk access and decoding: call only from a worker, never from the event loop.
    pub fn get_or_create_sized(&self, path: &Path, dimension: u32) -> Option<RawRgbaImage> {
        let metadata = std::fs::metadata(path).ok()?;
        // Ignore files larger than 40 MB for thumbnailing to keep UI fast
        if metadata.len() > 40 * 1024 * 1024 {
            return None;
        }
        let stamp = (
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            metadata.len(),
        );
        let key = (path.to_path_buf(), dimension.clamp(1, 64));

        if let Ok(mut lock) = self.cache.lock() {
            lock.clock += 1;
            let used = lock.clock;
            if let Some(entry) = lock.entries.get_mut(&key) {
                if entry.stamp == stamp {
                    entry.used = used;
                    return entry.image.clone();
                }
            }
        }

        let thumb = load_image_thumbnail_raw(path, key.1);

        if let Ok(mut lock) = self.cache.lock() {
            if let Some(old) = lock.entries.remove(&key) {
                lock.bytes -= thumbnail_bytes(&old.image);
            }
            lock.bytes += thumbnail_bytes(&thumb);
            lock.clock += 1;
            let used = lock.clock;
            lock.entries.insert(
                key,
                CachedThumbnail {
                    stamp,
                    image: thumb.clone(),
                    used,
                },
            );
            while lock.bytes > 4 * 1024 * 1024 || lock.entries.len() > 600 {
                let oldest = lock
                    .entries
                    .iter()
                    .min_by_key(|(_, e)| e.used)
                    .map(|(key, _)| key.clone());
                if let Some(oldest) = oldest {
                    if let Some(old) = lock.entries.remove(&oldest) {
                        lock.bytes -= thumbnail_bytes(&old.image);
                    }
                }
            }
        }

        thumb
    }
}

fn thumbnail_bytes(image: &Option<RawRgbaImage>) -> usize {
    image.as_ref().map_or(0, |i| i.data.as_bytes().len())
}

pub(crate) fn project_logo_raw(root: &Path, cache: &ThumbnailCache) -> Option<RawRgbaImage> {
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
        .find_map(|relative| cache.get_or_create_sized(&root.join(relative), 64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_claimed_payload_is_rejected_before_decoder_allocation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.ico");
        let mut data = vec![0u8; 22];
        data[2] = 1;
        data[4] = 1;
        data[6] = 16;
        data[7] = 16;
        data[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
        data[18..22].copy_from_slice(&22u32.to_le_bytes());
        std::fs::write(&path, data).unwrap();
        assert!(load_raster_image_raw(&path).is_err());
        assert!(load_raster_thumb_raw(&path, 36).is_err());
    }

    #[test]
    fn svg_external_images_are_omitted_in_full_view_and_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("private.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&image)
            .unwrap();
        let svg = dir.path().join("external.svg");
        std::fs::write(&svg, format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><image href="{}" width="2" height="2"/></svg>"#, image.display())).unwrap();
        let (full, _, _) = load_svg_raw(&svg).unwrap();
        let thumb = load_svg_thumb_raw(&svg, 36).unwrap();
        assert!(full.data.as_bytes().iter().all(|byte| *byte == 0));
        assert!(thumb.data.as_bytes().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn svg_dtd_depth_and_compressed_input_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.svg");
        std::fs::write(&path, r#"<!DOCTYPE svg [<!ENTITY text "denied">]><svg xmlns="http://www.w3.org/2000/svg"><text>&text;</text></svg>"#).unwrap();
        assert!(safe_svg_tree(&path).is_err());
        std::fs::write(
            &path,
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\">{}{}</svg>",
                "<g>".repeat(66),
                "</g>".repeat(66)
            ),
        )
        .unwrap();
        assert!(safe_svg_tree(&path).is_err());
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&[b' '; 512]).unwrap();
        std::fs::write(&path, encoder.finish().unwrap()).unwrap();
        assert!(read_svg(&path, 128).is_err());
    }

    #[test]
    fn pixels_are_shared_across_worker_and_ui_without_copying() {
        let raw = RawRgbaImage::new(vec![255; 256 * 256 * 4], 256, 256);
        let pointer = raw.data.as_bytes().as_ptr() as usize;
        let copy = std::thread::spawn(move || raw.clone()).join().unwrap();
        assert_eq!(pointer, copy.data.as_bytes().as_ptr() as usize);
        let image = copy.to_slint_image();
        let pixels = image.to_rgba8().unwrap();
        assert_eq!(pointer, pixels.as_bytes().as_ptr() as usize);
    }

    #[test]
    fn oversized_raster_is_rejected_and_tall_svg_has_bounded_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let raster = dir.path().join("wide.png");
        image::RgbaImage::new(32_768, 1).save(&raster).unwrap();
        assert!(load_raster_image_raw(&raster).is_err());
        assert!(load_raster_thumb_raw(&raster, 36).is_err());
        let vector = dir.path().join("tall.svg");
        std::fs::write(&vector, r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1000000"><rect width="1" height="1000000"/></svg>"#).unwrap();
        let (raw, _, height) = load_svg_raw(&vector).unwrap();
        assert_eq!(height, 1_000_000);
        assert!(raw.width <= 4096 && raw.height <= 4096);
    }

    #[test]
    fn thumbnail_cache_respects_byte_budget_dimensions_and_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hot.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let cache = ThumbnailCache::new();
        let first = cache.get_or_create_sized(&path, 64).unwrap();
        let hit = cache.get_or_create_sized(&path, 64).unwrap();
        assert_eq!(first.data.as_bytes().as_ptr(), hit.data.as_bytes().as_ptr());
        assert_eq!(cache.get_or_create(&path).unwrap().width, 36);
        for index in 0..270 {
            let other = dir.path().join(format!("{index}.png"));
            std::fs::copy(&path, &other).unwrap();
            cache.get_or_create_sized(&other, 64).unwrap();
            cache.get_or_create_sized(&path, 64).unwrap();
        }
        {
            let store = cache.cache.lock().unwrap();
            assert!(store.bytes <= 4 * 1024 * 1024);
            assert!(store.entries.len() <= 600);
        }
        assert_eq!(
            first.data.as_bytes().as_ptr(),
            cache
                .get_or_create_sized(&path, 64)
                .unwrap()
                .data
                .as_bytes()
                .as_ptr()
        );
        image::RgbaImage::from_pixel(20, 10, image::Rgba([9, 8, 7, 255]))
            .save(&path)
            .unwrap();
        let changed = cache.get_or_create_sized(&path, 64).unwrap();
        assert_eq!((changed.width, changed.height), (64, 32));
        assert_eq!(&changed.data.as_bytes()[..4], &[9, 8, 7, 255]);
    }

    #[test]
    fn test_is_image_file_extensions() {
        assert!(is_image_file(Path::new("icon.png")));
        assert!(is_image_file(Path::new("photo.jpg")));
        assert!(is_image_file(Path::new("photo.jpeg")));
        assert!(is_image_file(Path::new("graphic.webp")));
        assert!(is_image_file(Path::new("anim.gif")));
        assert!(is_image_file(Path::new("logo.svg")));
        assert!(is_image_file(Path::new("app.ico")));
        assert!(is_image_file(Path::new("bitmap.bmp")));
        assert!(is_image_file(Path::new("scan.tiff")));
        assert!(is_image_file(Path::new("scan.tif")));
        assert!(is_image_file(Path::new("modern.avif")));
        assert!(is_image_file(Path::new("fast.qoi")));
        assert!(is_image_file(Path::new("old.tga")));

        assert!(!is_image_file(Path::new("main.rs")));
        assert!(!is_image_file(Path::new("README.md")));
        assert!(!is_image_file(Path::new("package.json")));
    }

    #[test]
    fn test_file_size_formatting() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_file_size(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[test]
    fn test_png_load_and_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("sample.png");

        // Create a 64x32 PNG image
        let img = image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 8) as u8, 128, 255])
        });
        img.save(&file_path).unwrap();

        let details = load_image_full(&file_path).unwrap();
        assert_eq!(details.width, 64);
        assert_eq!(details.height, 32);
        assert_eq!(details.format_name, "PNG");
        assert!(details.summary.contains("PNG"));
        assert!(details.summary.contains("64 × 32 px"));

        let cache = ThumbnailCache::new();
        let thumb1 = cache.get_or_create(&file_path);
        assert!(thumb1.is_some());
        let thumb_img = thumb1.unwrap();
        assert!(thumb_img.width <= 36);
        assert!(thumb_img.height <= 36);

        // Cached hit returns identical dimensions
        let thumb2 = cache.get_or_create(&file_path).unwrap();
        assert_eq!(thumb_img.width, thumb2.width);
        assert_eq!(thumb_img.height, thumb2.height);
    }

    #[test]
    fn test_svg_load_and_thumbnail() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("vector.svg");
        let svg_data = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80">
            <rect width="100" height="80" fill="#3498db"/>
            <circle cx="50" cy="40" r="30" fill="#e74c3c"/>
        </svg>"##;
        std::fs::write(&file_path, svg_data).unwrap();

        let details = load_image_full(&file_path).unwrap();
        assert_eq!(details.format_name, "SVG");
        assert!(details.width >= 100);
        assert!(details.summary.contains("SVG"));

        let thumb = load_image_thumbnail_raw(&file_path, 32);
        assert!(thumb.is_some());
        let t = thumb.unwrap();
        assert!(t.width <= 32);
        assert!(t.height <= 32);
    }
}
