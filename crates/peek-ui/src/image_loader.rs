use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result};

/// Decoded pixel buffer that is safely Send + Sync across worker and UI threads.
#[derive(Clone)]
pub struct RawRgbaImage {
    pub data: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}

impl RawRgbaImage {
    pub fn new(data: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            data: data.into(),
            width,
            height,
        }
    }

    /// Converts raw RGBA pixels into a Slint Image on the UI thread.
    pub fn to_slint_image(&self) -> slint::Image {
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &self.data,
            self.width,
            self.height,
        );
        slint::Image::from_rgba8(buffer)
    }
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
    let data =
        std::fs::read(path).with_context(|| format!("Error al leer SVG: {}", path.display()))?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt)
        .context("No se pudo interpretar el archivo SVG como gráfico vectorial")?;

    let size = tree.size();
    let native_w = size.width().round() as u32;
    let native_h = size.height().round() as u32;

    let target_w = if native_w < 512 && native_h < 512 {
        let scale = (512.0 / native_w.max(1) as f32).max(1.0);
        (native_w as f32 * scale).round() as u32
    } else {
        native_w.min(4096)
    };
    let target_h = ((target_w as f32 / size.width()) * size.height()).round() as u32;

    let target_w = target_w.max(1);
    let target_h = target_h.max(1);

    let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h)
        .context("No se pudo crear el buffer para el SVG")?;

    let sx = target_w as f32 / size.width();
    let sy = target_h as f32 / size.height();
    let transform = tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let raw = RawRgbaImage::new(pixmap.data().to_vec(), pixmap.width(), pixmap.height());

    Ok((raw, native_w.max(target_w), native_h.max(target_h)))
}

/// Loads a small thumbnail of an SVG as raw RGBA pixels.
pub fn load_svg_thumb_raw(path: &Path, max_dim: u32) -> Result<RawRgbaImage> {
    let data = std::fs::read(path)?;
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &opt)?;
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
    Ok(RawRgbaImage::new(
        pixmap.data().to_vec(),
        pixmap.width(),
        pixmap.height(),
    ))
}

/// Loads raster images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF, QOI, TGA, etc.) into raw RGBA pixels.
pub fn load_raster_image_raw(path: &Path) -> Result<(RawRgbaImage, u32, u32, &'static str)> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"))
    {
        if let Ok(file) = std::fs::File::open(path) {
            if let Ok(directory) = ico::IconDir::read(file) {
                if let Some(entry) = directory.entries().iter().max_by_key(|e| e.width()) {
                    if let Ok(icon) = entry.decode() {
                        let w = icon.width();
                        let h = icon.height();
                        let raw = RawRgbaImage::new(icon.rgba_data().to_vec(), w, h);
                        return Ok((raw, w, h, "ICO"));
                    }
                }
            }
        }
    }

    let reader = image::ImageReader::open(path)
        .with_context(|| format!("No se pudo abrir el archivo: {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("Formato de imagen no reconocido: {}", path.display()))?;

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

    let decoded = reader
        .decode()
        .with_context(|| format!("No se pudo decodificar la imagen: {}", path.display()))?;

    let width = decoded.width();
    let height = decoded.height();
    let rgba = decoded.to_rgba8();

    let raw = RawRgbaImage::new(rgba.into_raw(), width, height);

    Ok((raw, width, height, format_name))
}

/// Loads a small thumbnail of a raster image as raw RGBA pixels.
pub fn load_raster_thumb_raw(path: &Path, max_dim: u32) -> Result<RawRgbaImage> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"))
    {
        if let Ok(file) = std::fs::File::open(path) {
            if let Ok(directory) = ico::IconDir::read(file) {
                if let Some(entry) = directory
                    .entries()
                    .iter()
                    .min_by_key(|e| (e.width() as i32 - max_dim as i32).abs())
                {
                    if let Ok(icon) = entry.decode() {
                        let w = icon.width();
                        let h = icon.height();
                        let raw = RawRgbaImage::new(icon.rgba_data().to_vec(), w, h);
                        return Ok(raw);
                    }
                }
            }
        }
    }

    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let decoded = reader.decode()?;
    let thumb = decoded.thumbnail(max_dim, max_dim);
    let rgba = thumb.to_rgba8();
    Ok(RawRgbaImage::new(
        rgba.into_raw(),
        thumb.width(),
        thumb.height(),
    ))
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

type CachedThumbnail = (SystemTime, Option<RawRgbaImage>);
type ThumbnailMap = Arc<Mutex<HashMap<PathBuf, CachedThumbnail>>>;

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
        let metadata = std::fs::metadata(path).ok()?;
        // Ignore files larger than 40 MB for thumbnailing to keep UI fast
        if metadata.len() > 40 * 1024 * 1024 {
            return None;
        }
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        if let Ok(lock) = self.cache.lock() {
            if let Some((cached_mtime, cached_thumb)) = lock.get(path) {
                if *cached_mtime == mtime {
                    return cached_thumb.clone();
                }
            }
        }

        let thumb = load_image_thumbnail_raw(path, 36);

        if let Ok(mut lock) = self.cache.lock() {
            if lock.len() > 600 {
                lock.clear();
            }
            lock.insert(path.to_path_buf(), (mtime, thumb.clone()));
        }

        thumb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
