use crate::themes::ColorRgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    Normal = 0,
    Heading1 = 1,
    Heading2 = 2,
    Heading3 = 3,
    Blockquote = 4,
    Divider = 5,
    CodeFence = 6,
    ListItem = 7,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub text: String,
    pub color: ColorRgb,
    pub bold: bool,
    pub italic: bool,
}

impl HighlightSpan {
    pub fn new(text: impl Into<String>, color: ColorRgb) -> Self {
        Self {
            text: text.into(),
            color,
            bold: false,
            italic: false,
        }
    }

    pub fn bold(text: impl Into<String>, color: ColorRgb) -> Self {
        Self {
            text: text.into(),
            color,
            bold: true,
            italic: false,
        }
    }

    pub fn italic(text: impl Into<String>, color: ColorRgb) -> Self {
        Self {
            text: text.into(),
            color,
            bold: false,
            italic: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedLine {
    pub line_number: usize,
    pub line_type: LineType,
    pub spans: Vec<HighlightSpan>,
    pub is_markdown: bool,
}

impl HighlightedLine {
    pub fn plain(line_number: usize, text: String, color: ColorRgb) -> Self {
        Self {
            line_number,
            line_type: LineType::Normal,
            spans: vec![HighlightSpan::new(text, color)],
            is_markdown: false,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct LineContext {
    pub dotenv_quote: Option<char>,
    pub in_code_block: bool,
    pub code_block_lang: Option<String>,
}

pub trait LanguageExtension: Send + Sync {
    /// Unique identifier for this extension, e.g. "rust", "markdown", "nextjs".
    fn id(&self) -> &'static str;

    /// Human readable name, e.g. "Rust", "Markdown", "Next.js / React (TSX)".
    fn name(&self) -> &'static str;

    /// File extensions handled, without leading dot, e.g. ["rs"].
    fn file_extensions(&self) -> &'static [&'static str];

    /// Special exact filenames, e.g. ["next.config.js", "package.json"].
    fn special_filenames(&self) -> &'static [&'static str] {
        &[]
    }

    fn matches_filename(&self, filename: &str) -> bool {
        self.special_filenames()
            .iter()
            .any(|name| name.eq_ignore_ascii_case(filename))
    }

    /// Whether this extension represents Markdown format.
    fn is_markdown(&self) -> bool {
        false
    }

    /// Highlight and format a single line of text with context.
    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        ctx: &mut LineContext,
    ) -> HighlightedLine;
}
