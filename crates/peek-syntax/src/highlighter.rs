use crate::languages::{HighlightSpan, HighlightedLine, LineType};
use crate::themes::ColorRgb;
use peek_core::TextBuffer;
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

pub struct SyntaxHighlighter {
    pub syntax_set: SyntaxSet,
    pub theme_set: ThemeSet,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
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
            .and_then(|ext| self.syntax_set.find_syntax_by_extension(ext))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        // Default to a dark theme bundled in syntect
        let theme = self
            .theme_set
            .themes
            .get("base16-ocean.dark")
            .or_else(|| self.theme_set.themes.values().next());

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
            if let Ok(ranges) = highlighter.highlight_line(&line_with_nl, &self.syntax_set) {
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
}
