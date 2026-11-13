use crate::languages::{HighlightSpan, HighlightedLine, LineType};
use crate::themes::ColorRgb;
use peek_core::TextBuffer;
use std::path::Path;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Grammars and themes deserialize on first use, so opening a PDF, an image or a
/// spreadsheet never pays for them.
#[derive(Default)]
pub struct SyntaxHighlighter {
    syntax_set: OnceLock<SyntaxSet>,
    theme_set: OnceLock<ThemeSet>,
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn syntax_set(&self) -> &SyntaxSet {
        self.syntax_set
            .get_or_init(SyntaxSet::load_defaults_newlines)
    }

    fn theme_set(&self) -> &ThemeSet {
        self.theme_set.get_or_init(ThemeSet::load_defaults)
    }

    /// Highlights a range of visible lines in the buffer using Syntect.
    pub fn highlight_viewport(
        &self,
        buffer: &TextBuffer,
        path: Option<&Path>,
        start_line: usize,
        count: usize,
    ) -> Vec<HighlightedLine> {
        let total_lines = buffer.line_count();
        if total_lines == 0 || start_line >= total_lines {
            return Vec::new();
        }

        let end_line = (start_line + count).min(total_lines);

        // Find syntax based on file extension
        let syntax = path
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(|ext| self.syntax_set().find_syntax_by_extension(ext))
            .unwrap_or_else(|| self.syntax_set().find_syntax_plain_text());

        // Default to a dark theme bundled in syntect
        let theme = self
            .theme_set()
            .themes
            .get("base16-ocean.dark")
            .or_else(|| self.theme_set().themes.values().next());

        let theme = match theme {
            Some(t) => t,
            None => {
                // Fallback: unhighlighted plain lines
                return (start_line..end_line)
                    .map(|idx| HighlightedLine {
                        line_number: idx + 1,
                        line_type: LineType::Normal,
                        spans: vec![HighlightSpan::new(
                            buffer.line_trimmed(idx).unwrap_or_default(),
                            ColorRgb::new(212, 212, 216),
                        )],
                        is_markdown: false,
                    })
                    .collect();
            }
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut result = Vec::with_capacity(end_line - start_line);

        // Viewport-local parsing keeps deep scroll independent of document length.
        // Multiline syntax beginning outside this viewport may have approximate colors.

        // Highlight visible lines
        for i in start_line.saturating_sub(64)..end_line {
            let line_content = buffer.line_trimmed(i).unwrap_or_default();
            let mut line_with_nl = line_content.clone();
            line_with_nl.push('\n');

            let mut spans = Vec::new();
            if let Ok(ranges) = highlighter.highlight_line(&line_with_nl, self.syntax_set()) {
                if i < start_line {
                    continue;
                }
                for (style, text) in ranges {
                    let clean_text = text.trim_end_matches(&['\r', '\n'][..]);
                    if !clean_text.is_empty() {
                        spans.push(HighlightSpan::new(
                            clean_text.to_string(),
                            ColorRgb::new(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            ),
                        ));
                    }
                }
            }

            if spans.is_empty() && !line_content.is_empty() {
                spans.push(HighlightSpan::new(
                    line_content,
                    ColorRgb::new(212, 212, 216),
                ));
            }

            result.push(HighlightedLine {
                line_number: i + 1,
                line_type: LineType::Normal,
                spans,
                is_markdown: false,
            });
        }

        result
    }

    /// Highlights an embedded fenced code snippet by language token (e.g. "rust", "ts", "json").
    pub fn highlight_code_snippet(
        &self,
        lang: Option<&str>,
        code: &str,
    ) -> Vec<Vec<HighlightSpan>> {
        let syntax = lang
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .and_then(|token| {
                self.syntax_set()
                    .find_syntax_by_token(token)
                    .or_else(|| self.syntax_set().find_syntax_by_extension(token))
            })
            .unwrap_or_else(|| self.syntax_set().find_syntax_plain_text());

        let theme = self
            .theme_set()
            .themes
            .get("base16-ocean.dark")
            .or_else(|| self.theme_set().themes.values().next());

        let mut out = Vec::new();
        let Some(theme) = theme else {
            for line in code.lines() {
                out.push(vec![HighlightSpan::new(line, ColorRgb::new(212, 212, 216))]);
            }
            return out;
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        for line in code.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let mut line_with_nl = String::with_capacity(line.len() + 1);
            line_with_nl.push_str(line);
            line_with_nl.push('\n');

            let mut spans = Vec::new();
            if let Ok(ranges) = highlighter.highlight_line(&line_with_nl, self.syntax_set()) {
                for (style, text) in ranges {
                    let clean = text.trim_end_matches(['\r', '\n']);
                    if !clean.is_empty() {
                        spans.push(HighlightSpan::new(
                            clean,
                            ColorRgb::new(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            ),
                        ));
                    }
                }
            }
            if spans.is_empty() && !line.is_empty() {
                spans.push(HighlightSpan::new(line, ColorRgb::new(212, 212, 216)));
            }
            out.push(spans);
        }
        // Trim a single trailing empty line produced by trailing newline in fenced blocks.
        if code.ends_with('\n') && out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out
    }
}
