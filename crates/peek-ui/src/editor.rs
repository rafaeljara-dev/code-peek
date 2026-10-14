use std::ops::Range;

use peek_core::TextBuffer;
use peek_syntax::{ColorRgb, HighlightSpan};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Character offsets address the rope; display columns address grapheme cells.
#[derive(Clone, Default, Debug)]
pub struct EditorState {
    pub cursor: usize,
    pub anchor: usize,
    pub left_column: usize,
    preferred_column: Option<usize>,
}

pub struct EditSnapshot {
    pub buffer: TextBuffer,
    pub editor: EditorState,
}

impl EditSnapshot {
    pub fn new(buffer: TextBuffer, editor: &EditorState) -> Self {
        Self {
            buffer,
            editor: editor.clone(),
        }
    }
}

impl EditorState {
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    pub fn clamp(&mut self, buffer: &TextBuffer) {
        self.cursor = self.cursor.min(buffer.len_chars());
        self.anchor = self.anchor.min(buffer.len_chars());
    }

    pub fn position(&self, buffer: &TextBuffer) -> (usize, usize) {
        let cursor = self.cursor.min(buffer.len_chars());
        let line = buffer.char_to_line(cursor);
        let prefix = buffer
            .rope
            .slice(buffer.line_to_char(line)..cursor)
            .to_string();
        (line, display_width(&prefix))
    }

    pub fn point(&mut self, buffer: &TextBuffer, line: usize, column: usize, extend: bool) {
        let line = line.min(buffer.line_count() - 1);
        self.cursor = buffer.line_to_char(line)
            + char_at_column(&buffer.line_trimmed(line).unwrap_or_default(), column);
        if !extend {
            self.anchor = self.cursor;
        }
        self.preferred_column = None;
    }

    pub fn selected_text(&self, buffer: &TextBuffer) -> String {
        buffer.rope.slice(self.selection()).to_string()
    }

    fn replace(&mut self, buffer: &mut TextBuffer, text: &str) {
        let range = self.selection();
        if buffer.rope.slice(range.clone()).chars().eq(text.chars()) {
            self.cursor = range.end;
            self.anchor = self.cursor;
            return;
        }
        buffer.delete(range.clone());
        if !text.is_empty() {
            buffer.insert(range.start, text);
        }
        self.cursor = range.start + text.chars().count();
        self.anchor = self.cursor;
        self.preferred_column = None;
    }

    /// Native input delivers committed IME text and clipboard content here.
    pub fn command(
        &mut self,
        buffer: &mut TextBuffer,
        action: &str,
        text: &str,
        shift: bool,
        control: bool,
    ) {
        self.clamp(buffer);
        let (line, column) = self.position(buffer);
        let start = buffer.line_to_char(line);
        let source = buffer.line_trimmed(line).unwrap_or_default();
        match action {
            "insert" | "paste" => {
                let normalized = text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', buffer.line_ending.as_str());
                if action == "insert" && !shift && self.selection().is_empty() {
                    if let Some(close) = match normalized.as_str() {
                        "(" => Some(')'),
                        "[" => Some(']'),
                        "{" => Some('}'),
                        _ => None,
                    } {
                        let next = buffer.rope.get_char(self.cursor);
                        if next.is_none_or(|c| c.is_whitespace() || ")]},;".contains(c)) {
                            self.replace(buffer, &format!("{normalized}{close}"));
                            self.cursor -= 1;
                            self.anchor = self.cursor;
                            return;
                        }
                    }
                    if normalized.chars().count() == 1
                        && ")]}".contains(normalized.as_str())
                        && buffer
                            .rope
                            .get_char(self.cursor)
                            .is_some_and(|c| c.to_string() == normalized)
                    {
                        self.cursor += 1;
                        self.anchor = self.cursor;
                        return;
                    }
                }
                self.replace(buffer, &normalized);
            }
            "newline" => {
                let before: String = buffer.rope.slice(start..self.cursor).chars().collect();
                let indent: String = source
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let extra = if before.trim_end().ends_with(['{', '[', '(']) {
                    "    "
                } else {
                    ""
                };
                let paired = before
                    .chars()
                    .next_back()
                    .zip(buffer.rope.get_char(self.cursor))
                    .is_some_and(|(a, b)| matches!((a, b), ('{', '}') | ('[', ']') | ('(', ')')));
                let tail = if paired {
                    format!("{}{indent}", buffer.line_ending.as_str())
                } else {
                    String::new()
                };
                self.replace(
                    buffer,
                    &format!("{}{indent}{extra}{tail}", buffer.line_ending.as_str()),
                );
                self.cursor -= tail.chars().count();
                self.anchor = self.cursor;
            }
            "backspace" | "delete" | "cut" => {
                if self.selection().is_empty() {
                    if action == "cut" {
                        return;
                    }
                    if action == "backspace"
                        && !control
                        && self.cursor > 0
                        && buffer
                            .rope
                            .get_char(self.cursor - 1)
                            .zip(buffer.rope.get_char(self.cursor))
                            .is_some_and(|(a, b)| {
                                matches!((a, b), ('{', '}') | ('[', ']') | ('(', ')'))
                            })
                    {
                        self.anchor = self.cursor - 1;
                        self.cursor += 1;
                        self.replace(buffer, "");
                        return;
                    }
                    self.anchor = if action == "backspace" {
                        previous_boundary(buffer, self.cursor, control)
                    } else {
                        next_boundary(buffer, self.cursor, control)
                    };
                }
                self.replace(buffer, "");
            }
            "left" | "right" => {
                let range = self.selection();
                self.cursor = if !shift && !range.is_empty() {
                    if action == "left" {
                        range.start
                    } else {
                        range.end
                    }
                } else if action == "left" {
                    previous_boundary(buffer, self.cursor, control)
                } else {
                    next_boundary(buffer, self.cursor, control)
                };
                self.preferred_column = None;
            }
            "up" | "down" | "page-up" | "page-down" => {
                let goal = self.preferred_column.unwrap_or(column);
                let step = if action.starts_with("page-") {
                    text.parse().unwrap_or(25)
                } else {
                    1
                };
                let target = if action.ends_with("up") {
                    line.saturating_sub(step)
                } else {
                    (line + step).min(buffer.line_count() - 1)
                };
                self.cursor = buffer.line_to_char(target)
                    + char_at_column(&buffer.line_trimmed(target).unwrap_or_default(), goal);
                self.preferred_column = Some(goal);
            }
            "home" => {
                self.cursor = if control { 0 } else { start };
                self.preferred_column = None;
            }
            "end" => {
                self.cursor = if control {
                    buffer.len_chars()
                } else {
                    start + source.chars().count()
                };
                self.preferred_column = None;
            }
            "select-all" => {
                self.anchor = 0;
                self.cursor = buffer.len_chars();
                return;
            }
            "select-word" => {
                let chars: Vec<_> = source.chars().collect();
                let mut from = (self.cursor - start).min(chars.len());
                let mut to = from;
                while from > 0 && word_char(chars[from - 1]) {
                    from -= 1;
                }
                while to < chars.len() && word_char(chars[to]) {
                    to += 1;
                }
                self.anchor = start + from;
                self.cursor = start + to;
                return;
            }
            "indent" => {
                if self.selection().is_empty() && !shift {
                    self.replace(buffer, &" ".repeat(4 - column % 4));
                    return;
                }
                self.transform_lines(buffer, if shift { "outdent" } else { "indent" }, text);
                return;
            }
            "comment" => {
                self.transform_lines(buffer, "comment", text);
                return;
            }
            "duplicate" => {
                let content = buffer.rope.line(line).to_string();
                let end = start + content.chars().count();
                let insertion = if content.ends_with('\n') {
                    content
                } else {
                    format!("{}{content}", buffer.line_ending.as_str())
                };
                buffer.insert(end, &insertion);
                self.cursor = end
                    + if insertion.starts_with('\n') || insertion.starts_with('\r') {
                        buffer.line_ending.as_str().chars().count()
                    } else {
                        0
                    };
                self.anchor = self.cursor;
                return;
            }
            _ => return,
        }
        if !shift
            && !matches!(
                action,
                "insert" | "paste" | "newline" | "delete" | "backspace" | "cut"
            )
        {
            self.anchor = self.cursor;
        }
    }

    fn transform_lines(&mut self, buffer: &mut TextBuffer, action: &str, prefix: &str) {
        let range = self.selection();
        let first = buffer.char_to_line(range.start);
        let mut last = buffer.char_to_line(range.end);
        if range.end > range.start && range.end == buffer.line_to_char(last) {
            last = last.saturating_sub(1);
        }
        let uncomment = action == "comment"
            && (first..=last).all(|line| {
                let text = buffer.line_trimmed(line).unwrap_or_default();
                text.trim().is_empty() || text.trim_start().starts_with(prefix)
            });
        for line in (first..=last).rev() {
            let source = buffer.line_trimmed(line).unwrap_or_default();
            let offset = buffer.line_to_char(line);
            let indent = source
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
            if action == "indent" {
                buffer.insert(offset, "    ");
            } else if action == "outdent" {
                let count = if source.starts_with('\t') {
                    1
                } else {
                    source.chars().take_while(|c| *c == ' ').take(4).count()
                };
                buffer.delete(offset..offset + count);
            } else if !prefix.is_empty() && !source.trim().is_empty() {
                if uncomment {
                    let mut count = prefix.chars().count();
                    if source.chars().nth(indent + count) == Some(' ') {
                        count += 1;
                    }
                    buffer.delete(offset + indent..offset + indent + count);
                } else {
                    buffer.insert(offset + indent, &format!("{prefix} "));
                }
            }
        }
        self.anchor = buffer.line_to_char(first);
        self.cursor = if last + 1 < buffer.line_count() {
            buffer.line_to_char(last + 1)
        } else {
            buffer.len_chars()
        };
        self.preferred_column = None;
    }
}

fn previous_boundary(buffer: &TextBuffer, cursor: usize, word: bool) -> usize {
    if cursor == 0 {
        return 0;
    }
    let line = buffer.char_to_line(cursor);
    let start = buffer.line_to_char(line);
    if cursor == start {
        return cursor.saturating_sub(if cursor >= 2 && buffer.rope.char(cursor - 2) == '\r' {
            2
        } else {
            1
        });
    }
    let text = buffer.rope.slice(start..cursor).to_string();
    if word {
        let chars: Vec<_> = text.chars().collect();
        let mut i = chars.len();
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let kind = word_char(chars[i - 1]);
            i -= 1;
            while i > 0 && !chars[i - 1].is_whitespace() && word_char(chars[i - 1]) == kind {
                i -= 1;
            }
        }
        start + i
    } else {
        cursor
            - text
                .graphemes(true)
                .next_back()
                .map_or(1, |s| s.chars().count())
    }
}

fn next_boundary(buffer: &TextBuffer, cursor: usize, word: bool) -> usize {
    if cursor >= buffer.len_chars() {
        return buffer.len_chars();
    }
    let line = buffer.char_to_line(cursor);
    let end = buffer.line_to_char(line)
        + buffer
            .line_trimmed(line)
            .unwrap_or_default()
            .chars()
            .count();
    if cursor >= end {
        return (cursor
            + if buffer.rope.char(cursor) == '\r' {
                2
            } else {
                1
            })
        .min(buffer.len_chars());
    }
    let text = buffer.rope.slice(cursor..end).to_string();
    if word {
        let chars: Vec<_> = text.chars().collect();
        let mut i = 1;
        while i < chars.len()
            && !chars[i].is_whitespace()
            && word_char(chars[i]) == word_char(chars[0])
        {
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        cursor + i
    } else {
        cursor + text.graphemes(true).next().map_or(1, |s| s.chars().count())
    }
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn display_width(text: &str) -> usize {
    text.graphemes(true)
        .fold(0, |col, g| col + cell_width(g, col))
}

fn cell_width(g: &str, column: usize) -> usize {
    if g == "\t" {
        4 - column % 4
    } else {
        UnicodeWidthStr::width(g).max(1)
    }
}

pub fn char_at_column(text: &str, goal: usize) -> usize {
    let (mut column, mut offset) = (0, 0);
    for g in text.graphemes(true) {
        let width = cell_width(g, column);
        if goal < column + width {
            return if goal - column > width / 2 {
                offset + g.chars().count()
            } else {
                offset
            };
        }
        offset += g.chars().count();
        column += width;
    }
    offset
}

pub struct DisplaySpan {
    pub text: String,
    pub column: usize,
    pub color: ColorRgb,
}

/// Only retain tokens intersecting the horizontal viewport. Never change source text.
pub fn display_spans(
    source: &str,
    spans: &[HighlightSpan],
    left: usize,
    columns: usize,
) -> Vec<DisplaySpan> {
    let mut result = Vec::<DisplaySpan>::new();
    let (mut column, mut byte, mut token, mut token_end) =
        (0, 0, 0, spans.first().map_or(0, |s| s.text.len()));
    for g in source.graphemes(true) {
        if column >= left + columns {
            break;
        }
        while byte >= token_end && token + 1 < spans.len() {
            token += 1;
            token_end += spans[token].text.len();
        }
        let width = cell_width(g, column);
        let color = spans
            .get(token)
            .map_or(ColorRgb::new(218, 229, 240), |s| s.color);
        if column >= left {
            let display = if g == "\t" {
                " ".repeat(width)
            } else {
                g.to_owned()
            };
            if let Some(last) = result.last_mut().filter(|s| s.color == color) {
                last.text.push_str(&display);
            } else {
                result.push(DisplaySpan {
                    text: display,
                    column: column - left,
                    color,
                });
            }
        }
        column += width;
        byte += g.len();
    }
    result
}

/// Literal per-line matches, with source offsets retained through Unicode case folding.
pub fn matches(buffer: &TextBuffer, query: &str, case: bool, whole: bool) -> Vec<Range<usize>> {
    if query.is_empty() || query.contains(['\n', '\r']) {
        return Vec::new();
    }
    let needle = if case {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    let mut result = Vec::new();
    for line in 0..buffer.line_count() {
        let source = buffer.line_trimmed(line).unwrap_or_default();
        let chars: Vec<char> = source.chars().collect();
        let mut folded = String::new();
        let mut boundaries = Vec::with_capacity(chars.len() + 1);
        boundaries.push(0);
        for &ch in &chars {
            if case {
                folded.push(ch);
            } else {
                folded.extend(ch.to_lowercase());
            }
            boundaries.push(folded.len());
        }
        let base = buffer.line_to_char(line);
        for (byte, _) in folded.match_indices(&needle) {
            let (Ok(start), Ok(end)) = (
                boundaries.binary_search(&byte),
                boundaries.binary_search(&(byte + needle.len())),
            ) else {
                continue;
            };
            if !whole
                || (start == 0 || !word_char(chars[start - 1]))
                    && (end == chars.len() || !word_char(chars[end]))
            {
                result.push(base + start..base + end);
            }
        }
    }
    result
}

pub fn comment_prefix(path: Option<&std::path::Path>) -> &'static str {
    match path
        .and_then(|p| p.extension())
        .and_then(|s| s.to_str())
        .unwrap_or("")
    {
        "py" | "sh" | "bash" | "ps1" | "toml" | "yaml" | "yml" | "env" | "rb" => "#",
        "sql" | "lua" => "--",
        "rs" | "js" | "jsx" | "ts" | "tsx" | "c" | "h" | "cpp" | "cs" | "java" | "go" | "swift"
        | "kt" | "slint" => "//",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphemes_crlf_selection_and_typing_are_lossless() {
        let mut buffer = TextBuffer::from_text("niño e\u{301}😀\r\nnext");
        let mut editor = EditorState {
            cursor: 8,
            anchor: 8,
            ..Default::default()
        };
        editor.command(&mut buffer, "backspace", "", false, false);
        assert_eq!(buffer.rope.to_string(), "niño e\u{301}\r\nnext");
        editor.command(&mut buffer, "backspace", "", false, false);
        assert_eq!(buffer.rope.to_string(), "niño \r\nnext");
        editor.command(&mut buffer, "end", "", false, false);
        editor.command(&mut buffer, "delete", "", false, false);
        assert_eq!(buffer.rope.to_string(), "niño next");
        editor.command(&mut buffer, "select-all", "", false, false);
        editor.command(&mut buffer, "paste", "á\nx", false, false);
        assert_eq!(buffer.rope.to_string(), "á\r\nx");
    }

    #[test]
    fn indentation_pairing_and_comments_preserve_line_endings() {
        let mut buffer = TextBuffer::from_text("fn main() {");
        let mut editor = EditorState::default();
        editor.command(&mut buffer, "end", "", false, true);
        editor.command(&mut buffer, "newline", "", false, false);
        editor.command(&mut buffer, "insert", "(", false, false);
        assert_eq!(buffer.rope.to_string(), "fn main() {\n    ()");
        editor.command(&mut buffer, "insert", ")", false, false);
        assert_eq!(buffer.rope.to_string(), "fn main() {\n    ()");
        editor.command(&mut buffer, "select-all", "", false, false);
        editor.command(&mut buffer, "comment", "//", false, false);
        assert_eq!(buffer.rope.to_string(), "// fn main() {\n    // ()");
        editor.command(&mut buffer, "comment", "//", false, false);
        assert_eq!(buffer.rope.to_string(), "fn main() {\n    ()");
    }

    #[test]
    fn unicode_find_has_exact_offsets_and_word_boundaries() {
        let buffer = TextBuffer::from_text("NiÑO niño_ niñó\r\nniño Niño");
        let found = matches(&buffer, "niño", false, true);
        assert_eq!(found.len(), 3);
        for range in found {
            assert_eq!(buffer.rope.slice(range).to_string().to_lowercase(), "niño");
        }
        assert_eq!(matches(&buffer, "niño", true, false).len(), 2);
        assert_eq!(display_width("a\t😀"), 6);
        assert_eq!(char_at_column("a\t😀", 4), 2);
    }

    #[test]
    fn horizontal_viewport_keeps_only_visible_tokens() {
        let source = "let\tanswer = 42;";
        let spans = [HighlightSpan::new(source, ColorRgb::new(1, 2, 3))];
        let visible = display_spans(source, &spans, 4, 6);
        assert_eq!(
            visible.iter().map(|s| s.text.as_str()).collect::<String>(),
            "answer"
        );
    }
}
