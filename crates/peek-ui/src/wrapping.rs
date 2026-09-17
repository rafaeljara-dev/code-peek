use peek_core::TextBuffer;
use peek_syntax::{ColorRgb, HighlightSpan};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub struct Span {
    pub text: String,
    pub color: ColorRgb,
}

pub struct Row {
    pub line: usize,
    pub continuation: bool,
    pub spans: Vec<Span>,
}

/// Break display rows, never the source. Grapheme clusters and token colors remain intact.
pub fn wrap(source: &str, highlights: &[HighlightSpan], columns: usize) -> Vec<Vec<Span>> {
    let columns = columns.max(4);
    let joined: String = highlights.iter().map(|s| s.text.as_str()).collect();
    let fallback = [HighlightSpan::new(
        source.to_owned(),
        ColorRgb::new(218, 229, 240),
    )];
    let highlights = if joined == source {
        highlights
    } else {
        &fallback
    };
    let mut rows = vec![Vec::<Span>::new()];
    let mut column = 0;
    for span in highlights {
        for grapheme in span.text.graphemes(true) {
            let display = if grapheme == "\t" {
                " ".repeat(4 - column % 4)
            } else {
                grapheme.to_owned()
            };
            // Tabs may straddle a wrap boundary; ordinary graphemes remain indivisible.
            let parts: Vec<&str> = if grapheme == "\t" {
                display.graphemes(true).collect()
            } else {
                vec![&display]
            };
            for part in parts {
                let width = UnicodeWidthStr::width(part);
                if column + width > columns && column > 0 {
                    rows.push(Vec::new());
                    column = 0;
                }
                let row = rows.last_mut().unwrap();
                if let Some(last) = row.last_mut().filter(|s| s.color == span.color) {
                    last.text.push_str(part);
                } else {
                    row.push(Span {
                        text: part.to_owned(),
                        color: span.color,
                    });
                }
                column += width;
            }
        }
    }
    rows
}

pub fn row_count(buffer: &TextBuffer, line: usize, columns: usize) -> usize {
    wrap(&buffer.line_trimmed(line).unwrap_or_default(), &[], columns).len()
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
    fn wrapping_preserves_unicode_source_colors_and_blank_lines() {
        let source = "abc😀defe\u{301}gh";
        let color = ColorRgb::new(1, 2, 3);
        let rows = wrap(source, &[HighlightSpan::new(source, color)], 5);
        let joined: String = rows.iter().flatten().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, source);
        assert!(rows.iter().all(|r| r
            .iter()
            .map(|s| UnicodeWidthStr::width(s.text.as_str()))
            .sum::<usize>()
            <= 5));
        assert!(rows.iter().flatten().all(|s| s.color == color));
        assert_eq!(wrap("", &[], 5).len(), 1);
        assert_eq!(wrap("\tX", &[], 4).len(), 2);
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
}
