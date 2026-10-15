use peek_core::TextBuffer;
use peek_syntax::{ColorRgb, HighlightSpan};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub struct Span {
    pub text: String,
    pub color: ColorRgb,
    pub bold: bool,
    pub italic: bool,
}

pub struct Row {
    pub line: usize,
    pub continuation: bool,
    pub line_type: i32,
    pub is_markdown: bool,
    pub spans: Vec<Span>,
}

/// Break display rows, never the source. Grapheme clusters and token colors remain intact.
#[cfg(test)]
pub fn wrap(
    source: &str,
    highlights: &[HighlightSpan],
    columns: usize,
    formatted: bool,
) -> Vec<Vec<Span>> {
    wrap_visible(source, highlights, columns, formatted, 0, usize::MAX).1
}

/// Returns the clamped first fragment and only the requested visible rows.
pub fn wrap_visible(
    source: &str,
    highlights: &[HighlightSpan],
    columns: usize,
    formatted: bool,
    skip: usize,
    count: usize,
) -> (usize, Vec<Vec<Span>>) {
    if count == 0 {
        return (skip, Vec::new());
    }
    let columns = columns.max(4);
    let fallback;
    let source_preserved = highlights
        .iter()
        .try_fold(source, |rest, span| rest.strip_prefix(span.text.as_str()))
        .is_some_and(str::is_empty);
    let highlights = if formatted || source_preserved {
        highlights
    } else {
        fallback = [HighlightSpan::new(source, ColorRgb::new(218, 229, 240))];
        &fallback
    };
    let mut rows = Vec::new();
    let mut row = Vec::<Span>::new();
    let mut fragment = 0usize;
    let mut column = 0;
    for span in highlights {
        for grapheme in span.text.graphemes(true) {
            let (part, repeat) = display_parts(grapheme, column);
            for _ in 0..repeat {
                if place(part, &mut column, columns) {
                    if fragment >= skip {
                        rows.push(std::mem::take(&mut row));
                        if rows.len() >= count {
                            return (skip, rows);
                        }
                    } else {
                        row.clear();
                    }
                    fragment += 1;
                }
                // Retain the last skipped row to preserve clamping beyond end-of-line.
                if let Some(last) = row.last_mut().filter(|s| {
                    s.color == span.color && s.bold == span.bold && s.italic == span.italic
                }) {
                    last.text.push_str(part);
                } else {
                    row.push(Span {
                        text: part.to_owned(),
                        color: span.color,
                        bold: span.bold,
                        italic: span.italic,
                    });
                }
            }
        }
    }
    rows.push(row);
    (skip.min(fragment), rows)
}

/// A tab expands to spaces up to the next 4-column stop; any other grapheme is one part.
fn display_parts(grapheme: &str, column: usize) -> (&str, usize) {
    if grapheme == "\t" {
        (" ", 4 - column % 4)
    } else {
        (grapheme, 1)
    }
}

/// Advance past one display part and report whether it starts a new visual row.
fn place(part: &str, column: &mut usize, columns: usize) -> bool {
    let width = UnicodeWidthStr::width(part);
    let wraps = *column + width > columns && *column > 0;
    *column = if wraps { width } else { *column + width };
    wraps
}

pub fn row_count(buffer: &TextBuffer, line: usize, columns: usize) -> usize {
    let source = buffer.line_trimmed(line).unwrap_or_default();
    let columns = columns.max(4);
    let (mut count, mut column) = (1, 0);
    for grapheme in source.graphemes(true) {
        let (part, repeat) = display_parts(grapheme, column);
        for _ in 0..repeat {
            if place(part, &mut column, columns) {
                count += 1;
            }
        }
    }
    count
}

/// Move through visual rows without building an index of the entire document.
pub fn advance(
    buffer: &TextBuffer,
    line: &mut usize,
    fragment: &mut usize,
    delta: i32,
    columns: usize,
) {
    for _ in 0..delta.unsigned_abs() {
        if delta > 0 {
            if *fragment + 1 < row_count(buffer, *line, columns) {
                *fragment += 1;
            } else if *line + 1 < buffer.line_count() {
                *line += 1;
                *fragment = 0;
            } else {
                break;
            }
        } else if *fragment > 0 {
            *fragment -= 1;
        } else if *line > 0 {
            *line -= 1;
            *fragment = row_count(buffer, *line, columns) - 1;
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_wrapping_matches_full_rows_and_bounds_minified_output() {
        for source in ["\t😀ab e\u{301}xyz\tlast", "", "\u{301}a", "abcdefghi"] {
            let spans = [HighlightSpan::new(source, ColorRgb::new(1, 2, 3))];
            let full = wrap(source, &spans, 4, false);
            let buffer = TextBuffer::from_text(source);
            assert_eq!(row_count(&buffer, 0, 4), full.len());
            for skip in 0..full.len() + 2 {
                let (first, visible) = wrap_visible(source, &spans, 4, false, skip, 2);
                assert_eq!(first, skip.min(full.len() - 1));
                let text = |rows: &[Vec<Span>]| {
                    rows.iter()
                        .map(|row| row.iter().map(|s| s.text.as_str()).collect::<String>())
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    text(&visible),
                    text(&full[first..(first + 2).min(full.len())])
                );
            }
        }
        let source = "x".repeat(4 * 1024 * 1024);
        let spans = [HighlightSpan::new(&source, ColorRgb::new(1, 2, 3))];
        let (_, visible) = wrap_visible(&source, &spans, 100, false, 0, 40);
        assert_eq!(visible.len(), 40);
        assert_eq!(
            visible
                .iter()
                .flatten()
                .map(|s| s.text.len())
                .sum::<usize>(),
            4000
        );
    }
    #[test]
    fn wrapping_preserves_unicode_source_colors_and_blank_lines() {
        let source = "abc😀defe\u{301}gh";
        let color = ColorRgb::new(1, 2, 3);
        let rows = wrap(source, &[HighlightSpan::new(source, color)], 5, false);
        let joined: String = rows.iter().flatten().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, source);
        assert!(rows.iter().all(|r| r
            .iter()
            .map(|s| UnicodeWidthStr::width(s.text.as_str()))
            .sum::<usize>()
            <= 5));
        assert!(rows.iter().flatten().all(|s| s.color == color));
        assert_eq!(wrap("", &[], 5, false).len(), 1);
        assert_eq!(wrap("\tX", &[], 4, false).len(), 2);
    }
    #[test]
    fn scrolling_reaches_continuations_and_previous_lines() {
        let buffer = TextBuffer::from_text("123456789012\nnext");
        let (mut line, mut fragment) = (0, 0);
        advance(&buffer, &mut line, &mut fragment, 2, 4);
        assert_eq!((line, fragment), (0, 2));
        advance(&buffer, &mut line, &mut fragment, 1, 4);
        assert_eq!((line, fragment), (1, 0));
        advance(&buffer, &mut line, &mut fragment, -2, 4);
        assert_eq!((line, fragment), (0, 1));
    }

    #[test]
    fn formatted_markdown_keeps_presentational_spans() {
        let source = "**important**";
        let color = ColorRgb::new(1, 2, 3);
        let rows = wrap(source, &[HighlightSpan::bold("important", color)], 80, true);
        assert_eq!(rows[0][0].text, "important");
        assert!(rows[0][0].bold);
    }
}
