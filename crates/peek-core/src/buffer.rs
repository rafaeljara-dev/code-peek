use anyhow::{bail, Context, Result};
use ropey::Rope;
use std::fs::File;
use std::io::BufReader;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub line_ending: LineEnding,
    pub has_bom: bool,
    pub is_dirty: bool,
    pub revision: u64,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::str::FromStr for TextBuffer {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_text(s))
    }
}

impl From<&str> for TextBuffer {
    fn from(s: &str) -> Self {
        Self::from_text(s)
    }
}

impl TextBuffer {
    /// Creates an empty text buffer.
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            path: None,
            line_ending: LineEnding::Lf,
            has_bom: false,
            is_dirty: false,
            revision: 0,
        }
    }

    /// Creates a buffer from an in-memory string.
    pub fn from_text(content: &str) -> Self {
        let has_bom = content.starts_with('\u{FEFF}');
        let clean_content = if has_bom {
            &content['\u{FEFF}'.len_utf8()..]
        } else {
            content
        };

        let line_ending = Self::detect_line_ending(clean_content);
        let rope = Rope::from_str(clean_content);

        Self {
            rope,
            path: None,
            line_ending,
            has_bom,
            is_dirty: false,
            revision: 0,
        }
    }

    /// Streams UTF-8 directly into the rope without a second full-file allocation.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("Failed to read metadata: {}", path.display()))?;
        if !metadata.is_file() {
            bail!("La ruta no es un archivo regular");
        }
        let mut rope = Rope::from_reader(BufReader::new(file))
            .context("No se pudo leer el archivo como texto UTF-8")?;
        let has_bom = rope.chars().next() == Some('\u{FEFF}');
        if has_bom {
            rope.remove(0..1);
        }
        let mut crlf = 0;
        let mut lf = 0;
        let mut previous = '\0';
        for ch in rope.chars() {
            if ch == '\0' {
                bail!("El archivo es binario; CodePeek muestra archivos de texto");
            }
            if ch == '\n' {
                if previous == '\r' {
                    crlf += 1;
                } else {
                    lf += 1;
                }
            }
            previous = ch;
        }
        let mut buffer = Self {
            rope,
            has_bom,
            line_ending: if crlf > lf {
                LineEnding::Crlf
            } else {
                LineEnding::Lf
            },
            ..Self::new()
        };
        buffer.path = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        buffer.is_dirty = false;
        buffer.revision = 0;

        Ok(buffer)
    }

    /// Detects predominant line ending (\r\n vs \n).
    pub fn detect_line_ending(content: &str) -> LineEnding {
        let mut crlf_count = 0usize;
        let mut lf_count = 0usize;

        let bytes = content.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                crlf_count += 1;
                i += 2;
            } else if bytes[i] == b'\n' {
                lf_count += 1;
                i += 1;
            } else {
                i += 1;
            }
        }

        if crlf_count > lf_count {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }

    /// Total number of lines in the buffer.
    #[inline]
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Total number of characters.
    #[inline]
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Total number of bytes.
    #[inline]
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Returns the text content of a line without trailing line endings (\r, \n).
    pub fn line_trimmed(&self, line_idx: usize) -> Option<String> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        let line_slice = self.rope.line(line_idx);
        let mut s = line_slice.to_string();
        while s.ends_with('\n') || s.ends_with('\r') {
            s.pop();
        }
        Some(s)
    }

    pub fn replace_line(&mut self, line_idx: usize, text: &str) -> Result<()> {
        if line_idx >= self.line_count() {
            bail!("La línea ya no existe");
        }
        if text.contains(['\r', '\n']) {
            bail!("Edita una línea cada vez");
        }
        let old = self.line_trimmed(line_idx).unwrap_or_default();
        if old == text {
            return Ok(());
        }
        let start = self.rope.line_to_char(line_idx);
        self.rope.remove(start..start + old.chars().count());
        self.rope.insert(start, text);
        self.is_dirty = true;
        self.revision += 1;
        Ok(())
    }

    /// Converts a line index to character index.
    #[inline]
    pub fn line_to_char(&self, line_idx: usize) -> usize {
        self.rope.line_to_char(line_idx)
    }

    /// Converts a character index to line index.
    #[inline]
    pub fn char_to_line(&self, char_idx: usize) -> usize {
        self.rope.char_to_line(char_idx)
    }

    /// Inserts text at a specific character index.
    pub fn insert(&mut self, char_idx: usize, text: &str) {
        let idx = char_idx.min(self.rope.len_chars());
        self.rope.insert(idx, text);
        self.is_dirty = true;
        self.revision += 1;
    }

    /// Removes a range of characters.
    pub fn delete(&mut self, range: Range<usize>) {
        let start = range.start.min(self.rope.len_chars());
        let end = range.end.min(self.rope.len_chars());
        if start < end {
            self.rope.remove(start..end);
            self.is_dirty = true;
            self.revision += 1;
        }
    }

    /// Exports the content string preserving BOM and original line endings.
    pub fn to_string_with_endings(&self) -> String {
        let mut output =
            String::with_capacity(self.rope.len_bytes() + if self.has_bom { 3 } else { 0 });
        if self.has_bom {
            output.push('\u{FEFF}');
        }

        // The rope retains each original line ending, including mixed-ending files.
        for chunk in self.rope.chunks() {
            output.push_str(chunk);
        }

        output
    }
}
