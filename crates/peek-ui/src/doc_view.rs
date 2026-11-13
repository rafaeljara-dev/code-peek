use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use peek_syntax::{
    CalloutKind, ColorRgb, ExtensionRegistry, HighlightSpan, RichBlock, RichDocument, RichSpan,
    TableAlign,
};
use slint::{ModelRc, VecModel};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::image_loader::{self, RawRgbaImage};
use crate::theme;
use crate::{DocBlockItem, DocHeadingItem, DocLinkItem, DocRowLine, DocSpanItem, DocTableRow};

pub const DOC_WINDOW_SIZE: usize = 200;
const MAX_DOC_IMAGES: usize = 32;
/// Reading measure of the document column at 100 % zoom, in logical pixels.
pub const DOC_MAX_WIDTH: f32 = 860.0;
/// Long code blocks only instantiate this many lines until the reader expands them.
const CODE_COLLAPSED_LINES: usize = 40;
const CODE_EXPANDED_LINES: usize = 800;
const TABLE_MIN_COL_EM: f32 = 5.0;
const TABLE_MAX_COL_EM: f32 = 32.0;

/// Geometry the wrapping needs: the scroll area width and the body font, in logical pixels.
#[derive(Clone, Copy)]
pub struct DocLayout {
    pub width: f32,
    pub font: f32,
    pub zoom: f32,
}

impl DocLayout {
    /// Ems of `font + extra` pixels that fit in the text column after `inset` pixels.
    fn ems(&self, extra: f32, inset: f32) -> f32 {
        let column = if self.width > 120.0 {
            (self.width - 56.0).min(DOC_MAX_WIDTH * self.zoom)
        } else {
            DOC_MAX_WIDTH * self.zoom
        };
        // 12 px of block padding plus a 4 % margin for the estimated glyph widths.
        ((column - 12.0 - inset) * 0.96 / (self.font + extra).max(6.0)).max(8.0)
    }
}

#[derive(Default)]
pub struct DocViewState {
    pub document: Option<RichDocument>,
    pub revision: u64,
    pub window_start: usize,
    pub anchor_block: Option<usize>,
    pub cursor_block: Option<usize>,
    pub find_query: String,
    pub find_matches: Vec<usize>,
    pub find_cursor: usize,
    pub images: HashMap<String, RawRgbaImage>,
    pub expanded_code: HashSet<usize>,
    /// The whole window must be rebuilt (content, layout or window position changed).
    pub dirty: bool,
    /// Only selection/match flags changed: rows are patched in place.
    pub flags_dirty: bool,
    /// Block the view should scroll to on the next publish.
    pub scroll_target: Option<usize>,
}

pub struct PreparedDocSpan {
    pub text: String,
    pub color: ColorRgb,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub link: String,
}

pub struct PreparedDocBlock {
    pub index: i32,
    pub kind: i32,
    pub level: i32,
    pub prefix: String,
    pub plain_text: String,
    pub lines: Vec<Vec<PreparedDocSpan>>,
    pub table_headers: Vec<String>,
    pub table_rows: Vec<Vec<String>>,
    /// Column widths in tenths of an em, shared by the header and every row.
    pub col_widths: Vec<i32>,
    /// 0 left, 1 center, 2 right.
    pub col_aligns: Vec<i32>,
    /// Code lines not instantiated; 0 when the block is shown whole.
    pub more_lines: i32,
    pub expanded: bool,
    pub image: Option<RawRgbaImage>,
    pub image_alt: String,
    pub links: Vec<(String, String)>,
    pub selected: bool,
    pub highlighted: bool,
}

pub struct PreparedDocHeading {
    pub title: String,
    pub level: i32,
    pub block_index: i32,
}

pub struct PreparedDocModels {
    pub blocks: Vec<PreparedDocBlock>,
    pub headings: Vec<PreparedDocHeading>,
    pub total_blocks: i32,
    pub window_start: i32,
    pub has_more_before: bool,
    pub has_more_after: bool,
}

impl PreparedDocModels {
    pub fn into_slint(self, light_mode: bool) -> (Vec<DocBlockItem>, Vec<DocHeadingItem>) {
        let blocks = self
            .blocks
            .into_iter()
            .map(|b| {
                let links = ModelRc::new(VecModel::from(
                    b.links
                        .into_iter()
                        .map(|(label, url)| DocLinkItem {
                            label: label.into(),
                            url: url.into(),
                        })
                        .collect::<Vec<_>>(),
                ));
                let lines = if b.lines.is_empty() {
                    ModelRc::default()
                } else {
                    ModelRc::new(VecModel::from(
                        b.lines
                            .into_iter()
                            .map(|row| DocRowLine {
                                spans: ModelRc::new(VecModel::from(
                                    row.into_iter()
                                        .map(|s| DocSpanItem {
                                            text: s.text.into(),
                                            color: theme::foreground(
                                                s.color.r, s.color.g, s.color.b, light_mode,
                                            ),
                                            bold: s.bold,
                                            italic: s.italic,
                                            strikethrough: s.strikethrough,
                                            code: s.code,
                                            link: s.link.into(),
                                        })
                                        .collect::<Vec<_>>(),
                                )),
                            })
                            .collect::<Vec<_>>(),
                    ))
                };
                let table_headers = if b.table_headers.is_empty() {
                    ModelRc::default()
                } else {
                    ModelRc::new(VecModel::from(
                        b.table_headers
                            .into_iter()
                            .map(slint::SharedString::from)
                            .collect::<Vec<_>>(),
                    ))
                };
                let table_rows = if b.table_rows.is_empty() {
                    ModelRc::default()
                } else {
                    ModelRc::new(VecModel::from(
                        b.table_rows
                            .into_iter()
                            .map(|row| DocTableRow {
                                cells: ModelRc::new(VecModel::from(
                                    row.into_iter()
                                        .map(slint::SharedString::from)
                                        .collect::<Vec<_>>(),
                                )),
                            })
                            .collect::<Vec<_>>(),
                    ))
                };
                let has_image = b.image.is_some();
                let image = RawRgbaImage::to_slint_or_default(b.image.as_ref());
                DocBlockItem {
                    index: b.index,
                    kind: b.kind,
                    level: b.level,
                    prefix: b.prefix.into(),
                    plain_text: b.plain_text.into(),
                    lines,
                    table_headers,
                    table_rows,
                    col_widths: ModelRc::new(VecModel::from(b.col_widths)),
                    col_aligns: ModelRc::new(VecModel::from(b.col_aligns)),
                    more_lines: b.more_lines,
                    expanded: b.expanded,
                    has_image,
                    image,
                    image_alt: b.image_alt.into(),
                    links,
                    selected: b.selected,
                    highlighted: b.highlighted,
                }
            })
            .collect();
        let headings = self
            .headings
            .into_iter()
            .map(|h| DocHeadingItem {
                title: h.title.into(),
                level: h.level,
                block_index: h.block_index,
            })
            .collect();
        (blocks, headings)
    }
}

impl DocViewState {
    pub fn clear(&mut self) {
        self.document = None;
        self.revision = 0;
        self.window_start = 0;
        self.anchor_block = None;
        self.cursor_block = None;
        self.find_query.clear();
        self.find_matches.clear();
        self.find_cursor = 0;
        self.images.clear();
        self.expanded_code.clear();
        self.scroll_target = None;
        self.dirty = true;
    }

    pub fn ensure_markdown_parsed(
        &mut self,
        source: &str,
        revision: u64,
        doc_path: Option<&Path>,
        project_root: Option<&Path>,
        syntax: &ExtensionRegistry,
    ) {
        if self.document.is_some() && self.revision == revision {
            return;
        }
        let doc = RichDocument::from_markdown(source, syntax);
        self.images.clear();
        if let Some(path) = doc_path {
            for block in &doc.blocks {
                if self.images.len() >= MAX_DOC_IMAGES {
                    break;
                }
                if let RichBlock::Image { src, .. } = block {
                    if !self.images.contains_key(src) {
                        if let Some(resolved) = resolve_local_image(path, project_root, src) {
                            if let Ok(details) = image_loader::load_image_full(&resolved) {
                                self.images.insert(src.clone(), details.raw);
                            }
                        }
                    }
                }
            }
        }
        self.document = Some(doc);
        self.revision = revision;
        self.window_start = 0;
        self.anchor_block = None;
        self.cursor_block = None;
        self.find_matches.clear();
        self.find_cursor = 0;
        self.expanded_code.clear();
        self.dirty = true;
    }

    pub fn set_docx_document(&mut self, doc: RichDocument, revision: u64) {
        self.images.clear();
        for (src, raw_bytes) in &doc.embedded_images {
            if self.images.len() >= MAX_DOC_IMAGES {
                break;
            }
            if let Ok(decoded) = image_loader::decode_raster_bytes(raw_bytes) {
                self.images.insert(src.clone(), decoded);
            }
        }
        self.document = Some(doc);
        self.revision = revision;
        self.window_start = 0;
        self.anchor_block = None;
        self.cursor_block = None;
        self.find_query.clear();
        self.find_matches.clear();
        self.find_cursor = 0;
        self.expanded_code.clear();
        self.dirty = true;
    }

    pub fn toggle_code_block(&mut self, index: usize) {
        if !self.expanded_code.remove(&index) {
            self.expanded_code.insert(index);
        }
        self.dirty = true;
    }

    pub fn select_block(&mut self, index: usize, extend: bool) {
        let Some(doc) = &self.document else {
            return;
        };
        if index >= doc.blocks.len() {
            return;
        }
        if extend {
            if self.anchor_block.is_none() {
                self.anchor_block = Some(index);
            }
            self.cursor_block = Some(index);
        } else {
            self.anchor_block = Some(index);
            self.cursor_block = Some(index);
        }
        self.flags_dirty = true;
    }

    pub fn select_all(&mut self) {
        let Some(doc) = &self.document else {
            return;
        };
        if !doc.blocks.is_empty() {
            self.anchor_block = Some(0);
            self.cursor_block = Some(doc.blocks.len() - 1);
            self.flags_dirty = true;
        }
    }

    pub fn selected_range(&self) -> Option<(usize, usize)> {
        let (a, b) = (self.anchor_block?, self.cursor_block?);
        Some((a.min(b), a.max(b)))
    }

    pub fn selected_text(&self) -> String {
        let Some(doc) = &self.document else {
            return String::new();
        };
        match self.selected_range() {
            Some((start, end)) => doc.blocks[start..=end.min(doc.blocks.len().saturating_sub(1))]
                .iter()
                .map(RichBlock::plain_text)
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
            None => doc.plain_text(),
        }
    }

    pub fn jump_to_block(&mut self, block_index: usize) {
        let Some(doc) = &self.document else {
            return;
        };
        if block_index >= doc.blocks.len() {
            return;
        }
        // Only move the window when the block is outside it; the view scrolls to the block.
        let visible = self.window_start..self.window_start + DOC_WINDOW_SIZE;
        if !visible.contains(&block_index) {
            let max_start = doc.blocks.len().saturating_sub(DOC_WINDOW_SIZE);
            self.window_start = block_index.saturating_sub(8).min(max_start);
            self.dirty = true;
        }
        self.anchor_block = Some(block_index);
        self.cursor_block = Some(block_index);
        self.scroll_target = Some(block_index);
        self.flags_dirty = true;
    }

    pub fn step_window(&mut self, forward: bool) {
        let Some(doc) = &self.document else {
            return;
        };
        if doc.blocks.len() <= DOC_WINDOW_SIZE {
            return;
        }
        let max_start = doc.blocks.len().saturating_sub(DOC_WINDOW_SIZE);
        let step = DOC_WINDOW_SIZE / 2;
        self.window_start = if forward {
            (self.window_start + step).min(max_start)
        } else {
            self.window_start.saturating_sub(step)
        };
        self.dirty = true;
    }

    pub fn find(&mut self, query: &str, forward: bool, case_sensitive: bool) -> String {
        let Some(doc) = &self.document else {
            return "Sin documento".into();
        };
        if query.is_empty() {
            self.find_query.clear();
            self.find_matches.clear();
            self.find_cursor = 0;
            self.flags_dirty = true;
            return String::new();
        }
        let needle = if case_sensitive {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        if self.find_query != query || self.find_matches.is_empty() {
            self.find_query = query.to_owned();
            self.find_matches = doc
                .blocks
                .iter()
                .enumerate()
                .filter_map(|(idx, block)| {
                    let text = block.plain_text();
                    let matched = if case_sensitive {
                        text.contains(&needle)
                    } else {
                        text.to_lowercase().contains(&needle)
                    };
                    matched.then_some(idx)
                })
                .collect();
            self.find_cursor = 0;
        } else if !self.find_matches.is_empty() {
            self.find_cursor = if forward {
                (self.find_cursor + 1) % self.find_matches.len()
            } else if self.find_cursor == 0 {
                self.find_matches.len() - 1
            } else {
                self.find_cursor - 1
            };
        }
        if let Some(&block_idx) = self.find_matches.get(self.find_cursor) {
            self.jump_to_block(block_idx);
            format!("{} de {}", self.find_cursor + 1, self.find_matches.len())
        } else {
            self.flags_dirty = true;
            "Sin coincidencias".into()
        }
    }

    /// `(selected, highlighted)` for every block of the current window.
    pub fn window_flags(&self) -> Vec<(bool, bool)> {
        let Some(doc) = &self.document else {
            return Vec::new();
        };
        let end = (self.window_start + DOC_WINDOW_SIZE).min(doc.blocks.len());
        let range = self.selected_range();
        (self.window_start..end)
            .map(|index| {
                (
                    range.is_some_and(|(a, b)| index >= a && index <= b),
                    self.find_matches.binary_search(&index).is_ok(),
                )
            })
            .collect()
    }

    pub fn prepare_models(&self, layout: DocLayout) -> PreparedDocModels {
        let Some(doc) = &self.document else {
            return PreparedDocModels {
                blocks: Vec::new(),
                headings: Vec::new(),
                total_blocks: 0,
                window_start: 0,
                has_more_before: false,
                has_more_after: false,
            };
        };
        let end = (self.window_start + DOC_WINDOW_SIZE).min(doc.blocks.len());
        let flags = self.window_flags();

        let blocks = doc.blocks[self.window_start..end]
            .iter()
            .zip(flags)
            .enumerate()
            .map(|(offset, (block, (selected, highlighted)))| {
                let index = self.window_start + offset;
                let mut prepared =
                    prepare_block(index, block, layout, selected, highlighted, &self.images);
                if let RichBlock::CodeBlock { lines, .. } = block {
                    let expanded = self.expanded_code.contains(&index);
                    let limit = if expanded {
                        CODE_EXPANDED_LINES
                    } else {
                        CODE_COLLAPSED_LINES
                    };
                    prepared.lines.truncate(limit);
                    prepared.expanded = expanded;
                    prepared.more_lines = lines.len().saturating_sub(CODE_COLLAPSED_LINES) as i32;
                }
                prepared
            })
            .collect();

        let headings = doc
            .headings
            .iter()
            .map(|h| PreparedDocHeading {
                title: h.title.clone(),
                level: h.level as i32,
                block_index: h.block_index as i32,
            })
            .collect();

        PreparedDocModels {
            blocks,
            headings,
            total_blocks: doc.blocks.len() as i32,
            window_start: self.window_start as i32,
            has_more_before: self.window_start > 0,
            has_more_after: end < doc.blocks.len(),
        }
    }
}

fn prepare_block(
    index: usize,
    block: &RichBlock,
    layout: DocLayout,
    selected: bool,
    highlighted: bool,
    images: &HashMap<String, RawRgbaImage>,
) -> PreparedDocBlock {
    let links: Vec<(String, String)> = block
        .links()
        .into_iter()
        .map(|url| (compact_link_label(&url), url))
        .collect();
    let default_fg = ColorRgb::new(226, 232, 240);
    let base = PreparedDocBlock {
        index: index as i32,
        kind: 0,
        level: 0,
        prefix: String::new(),
        plain_text: block.plain_text(),
        lines: Vec::new(),
        table_headers: Vec::new(),
        table_rows: Vec::new(),
        col_widths: Vec::new(),
        col_aligns: Vec::new(),
        more_lines: 0,
        expanded: false,
        image: None,
        image_alt: String::new(),
        links,
        selected,
        highlighted,
    };

    match block {
        RichBlock::Heading { level, spans } => {
            let extra = match level {
                1 => 11.0,
                2 => 7.0,
                3 => 3.0,
                _ => 1.0,
            };
            PreparedDocBlock {
                kind: 1,
                level: *level as i32,
                // Headings are bold: narrow the measure accordingly.
                lines: spans_to_lines_if_styled(spans, layout.ems(extra, 0.0) / 1.08, default_fg),
                ..base
            }
        }
        RichBlock::Paragraph { spans } => PreparedDocBlock {
            lines: spans_to_lines_if_styled(spans, layout.ems(1.0, 0.0), default_fg),
            ..base
        },
        RichBlock::Blockquote {
            callout,
            paragraphs,
        } => {
            let level = match callout {
                None => 0,
                Some(CalloutKind::Note) => 1,
                Some(CalloutKind::Tip) => 2,
                Some(CalloutKind::Important) => 3,
                Some(CalloutKind::Warning) => 4,
                Some(CalloutKind::Caution) => 5,
            };
            let mut merged = Vec::new();
            for (idx, p) in paragraphs.iter().enumerate() {
                if idx > 0 {
                    merged.push(RichSpan::plain("\n"));
                }
                merged.extend(p.iter().cloned());
            }
            PreparedDocBlock {
                kind: 2,
                level,
                prefix: callout.map(|k| k.label()).unwrap_or("").to_owned(),
                lines: spans_to_lines_if_styled(
                    &merged,
                    layout.ems(0.0, 36.0),
                    ColorRgb::new(203, 213, 225),
                ),
                ..base
            }
        }
        RichBlock::ListItem {
            depth,
            ordered_index,
            checked,
            spans,
        } => {
            let prefix = match checked {
                Some(true) => "☑".to_owned(),
                Some(false) => "☐".to_owned(),
                None => match ordered_index {
                    Some(n) => format!("{n}."),
                    None => "•".to_owned(),
                },
            };
            let inset = f32::from(*depth) * 22.0 + 40.0;
            PreparedDocBlock {
                kind: 3,
                level: *depth as i32,
                prefix,
                lines: spans_to_lines_if_styled(spans, layout.ems(1.0, inset), default_fg),
                ..base
            }
        }
        RichBlock::CodeBlock {
            lang,
            lines,
            raw_code,
        } => PreparedDocBlock {
            kind: 4,
            prefix: lang.as_deref().unwrap_or("código").to_owned(),
            plain_text: raw_code.clone(),
            lines: code_lines_to_prepared(&lines[..lines.len().min(CODE_EXPANDED_LINES)]),
            ..base
        },
        RichBlock::Table {
            alignments,
            headers,
            rows,
        } => {
            let table_headers: Vec<String> = headers.iter().map(|cell| join_spans(cell)).collect();
            let table_rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| row.iter().map(|cell| join_spans(cell)).collect())
                .collect();
            let columns = table_rows
                .iter()
                .map(Vec::len)
                .chain([table_headers.len()])
                .max()
                .unwrap_or(0);
            // One width per column, shared by every row, so the grid lines up.
            let col_widths = (0..columns)
                .map(|col| {
                    let header = table_headers.get(col).map_or(0.0, |h| text_em(h) * 1.08);
                    let body = table_rows
                        .iter()
                        .filter_map(|row| row.get(col))
                        .map(|cell| text_em(cell))
                        .fold(0.0f32, f32::max);
                    ((header.max(body) + 0.6).clamp(TABLE_MIN_COL_EM, TABLE_MAX_COL_EM) * 10.0)
                        .ceil() as i32
                })
                .collect();
            let col_aligns = (0..columns)
                .map(|col| match alignments.get(col) {
                    Some(TableAlign::Center) => 1,
                    Some(TableAlign::Right) => 2,
                    _ => 0,
                })
                .collect();
            PreparedDocBlock {
                kind: 5,
                table_headers,
                table_rows,
                col_widths,
                col_aligns,
                ..base
            }
        }
        RichBlock::Image { alt, src } => PreparedDocBlock {
            kind: 6,
            prefix: src.clone(),
            image: images.get(src).cloned(),
            image_alt: if alt.is_empty() {
                src.clone()
            } else {
                alt.clone()
            },
            ..base
        },
        RichBlock::ThematicBreak => PreparedDocBlock {
            kind: 7,
            plain_text: "---".into(),
            ..base
        },
    }
}

fn join_spans(spans: &[RichSpan]) -> String {
    let mut out = String::new();
    for span in spans {
        out.push_str(&span.text);
    }
    out
}

fn compact_link_label(url: &str) -> String {
    if url.chars().count() <= 48 {
        url.to_owned()
    } else {
        let mut short: String = url.chars().take(45).collect();
        short.push('…');
        short
    }
}

fn spans_to_lines_if_styled(
    spans: &[RichSpan],
    max_em: f32,
    default_fg: ColorRgb,
) -> Vec<Vec<PreparedDocSpan>> {
    let has_inline_style = spans.iter().any(|s| {
        s.bold || s.italic || s.strikethrough || s.code || s.link.is_some() || s.color.is_some()
    });
    if !has_inline_style {
        // Empty `lines` tells Slint to use native proportional `Text { wrap: word-wrap; }`.
        return Vec::new();
    }
    wrap_rich_spans(spans, max_em)
        .into_iter()
        .map(|line_spans| {
            line_spans
                .into_iter()
                .map(|span| {
                    let color = span.color.unwrap_or_else(|| {
                        if span.link.is_some() {
                            ColorRgb::new(96, 165, 250)
                        } else if span.code {
                            ColorRgb::new(56, 189, 248)
                        } else if span.bold {
                            ColorRgb::new(248, 250, 252)
                        } else if span.italic {
                            ColorRgb::new(251, 191, 36)
                        } else {
                            default_fg
                        }
                    });
                    PreparedDocSpan {
                        text: span.text,
                        color,
                        bold: span.bold,
                        italic: span.italic,
                        strikethrough: span.strikethrough,
                        code: span.code,
                        link: span.link.unwrap_or_default(),
                    }
                })
                .collect()
        })
        .collect()
}

fn code_lines_to_prepared(lines: &[Vec<HighlightSpan>]) -> Vec<Vec<PreparedDocSpan>> {
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| PreparedDocSpan {
                    text: span.text.clone(),
                    color: span.color,
                    bold: span.bold,
                    italic: span.italic,
                    strikethrough: false,
                    code: false,
                    link: String::new(),
                })
                .collect()
        })
        .collect()
}

/// Approximate advance of a character in a proportional UI font, in em units.
/// The controller thread has no font access, so line breaks are estimated from these
/// classes; the layout keeps a small safety margin on top.
pub fn char_em(ch: char) -> f32 {
    match ch {
        ' ' | '\u{a0}' => 0.28,
        'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '!' | '|' | '\'' | '`' | '¡' => 0.26,
        'f' | 't' | 'r' | '(' | ')' | '[' | ']' | '{' | '}' | '-' | '"' | '/' | '\\' => 0.35,
        'm' | 'w' => 0.80,
        'M' | 'W' | '@' | '%' => 0.90,
        '0'..='9' => 0.55,
        'A'..='Z' => 0.64,
        'a'..='z' => 0.53,
        _ if ch.is_uppercase() => 0.66,
        _ if UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]) as &str) >= 2 => 1.0,
        _ => 0.56,
    }
}

pub fn text_em(text: &str) -> f32 {
    text.chars().map(char_em).sum()
}

fn span_em(template: &RichSpan, text: &str) -> f32 {
    if template.code {
        // Monospace run, drawn one pixel smaller than the body.
        text.chars().count() as f32 * 0.55
    } else if template.bold {
        text_em(text) * 1.07
    } else {
        text_em(text)
    }
}

/// Inline code draws a padded chip; every styled run adds the row spacing.
fn chunk_overhead_em(template: &RichSpan) -> f32 {
    if template.code {
        0.9
    } else {
        0.15
    }
}

fn same_style(a: &RichSpan, b: &RichSpan) -> bool {
    a.bold == b.bold
        && a.italic == b.italic
        && a.strikethrough == b.strikethrough
        && a.code == b.code
        && a.link == b.link
        && a.color == b.color
}

struct RowBuilder {
    max_em: f32,
    used: f32,
    rows: Vec<Vec<RichSpan>>,
    current: Vec<RichSpan>,
}

impl RowBuilder {
    fn break_row(&mut self) {
        self.rows.push(std::mem::take(&mut self.current));
        self.used = 0.0;
    }

    fn cost(&self, template: &RichSpan, text: &str) -> f32 {
        let joins = self
            .current
            .last()
            .is_some_and(|last| same_style(last, template));
        span_em(template, text)
            + if joins {
                0.0
            } else {
                chunk_overhead_em(template)
            }
    }

    fn push(&mut self, template: &RichSpan, text: &str) {
        self.used += self.cost(template, text);
        match self.current.last_mut() {
            Some(last) if same_style(last, template) => last.text.push_str(text),
            _ => {
                let mut item = template.clone();
                item.text = text.to_owned();
                self.current.push(item);
            }
        }
    }

    fn place(&mut self, token: &str, template: &RichSpan) {
        if token.is_empty() {
            return;
        }
        if token.chars().all(char::is_whitespace) {
            if self.used == 0.0 {
                return;
            }
            if self.used + self.cost(template, token) > self.max_em {
                self.break_row();
                return;
            }
            self.push(template, token);
            return;
        }
        if self.used > 0.0 && self.used + self.cost(template, token) > self.max_em {
            self.break_row();
        }
        if self.cost(template, token) <= self.max_em {
            self.push(template, token);
            return;
        }
        // Long single token: break cleanly by grapheme cluster.
        for grapheme in token.graphemes(true) {
            if self.used > 0.0 && self.used + self.cost(template, grapheme) > self.max_em {
                self.break_row();
            }
            self.push(template, grapheme);
        }
    }
}

/// Wraps styled spans into rows no wider than `max_em` ems of the body font.
pub fn wrap_rich_spans(spans: &[RichSpan], max_em: f32) -> Vec<Vec<RichSpan>> {
    let mut builder = RowBuilder {
        max_em: max_em.max(8.0),
        used: 0.0,
        rows: Vec::new(),
        current: Vec::new(),
    };
    for span in spans {
        for piece in span.text.split_inclusive('\n') {
            let (text, has_newline) = match piece.strip_suffix('\n') {
                Some(t) => (t, true),
                None => (piece, false),
            };
            // Word-aware wrapping by splitting whitespace boundaries.
            let mut token_start = 0usize;
            for (idx, ch) in text.char_indices() {
                if ch.is_whitespace() {
                    builder.place(&text[token_start..idx], span);
                    builder.place(&text[idx..idx + ch.len_utf8()], span);
                    token_start = idx + ch.len_utf8();
                }
            }
            builder.place(&text[token_start..], span);
            if has_newline {
                builder.break_row();
            }
        }
    }
    if !builder.current.is_empty() || builder.rows.is_empty() {
        builder.rows.push(builder.current);
    }
    builder.rows
}

pub fn resolve_local_image(
    doc_path: &Path,
    project_root: Option<&Path>,
    src: &str,
) -> Option<PathBuf> {
    let trimmed = src.trim();
    if trimmed.is_empty()
        || trimmed.contains("://")
        || trimmed.starts_with("data:")
        || trimmed.starts_with(r"\\")
    {
        return None;
    }
    let clean = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let rel = Path::new(clean);
    if rel.is_absolute() {
        return None;
    }
    let parent = doc_path.parent()?;
    let candidate = parent.join(rel);
    let canonical = candidate.canonicalize().ok()?;
    let anchor = project_root
        .and_then(|root| root.canonicalize().ok())
        .or_else(|| parent.canonicalize().ok())?;
    if !canonical.starts_with(&anchor) {
        return None;
    }
    image_loader::is_image_file(&canonical).then_some(canonical)
}

pub enum LinkTarget {
    ExternalUrl(String),
    LocalFile(PathBuf),
}

pub fn classify_link(doc_path: Option<&Path>, url: &str) -> Option<LinkTarget> {
    let trimmed = url.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        return Some(LinkTarget::ExternalUrl(trimmed.to_owned()));
    }
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.contains("://") {
        return None;
    }
    let clean = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let rel = Path::new(clean);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        return None;
    }
    let base = doc_path?.parent()?;
    let candidate = base.join(rel);
    candidate
        .is_file()
        .then_some(LinkTarget::LocalFile(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_styled_spans_without_splitting_words_or_graphemes() {
        let spans = vec![
            RichSpan::plain("Hola "),
            RichSpan {
                text: "mundoñ".into(),
                bold: true,
                ..RichSpan::plain("")
            },
            RichSpan::plain(" bonito"),
        ];
        let wrapped = wrap_rich_spans(&spans, 8.0);
        assert!(wrapped.len() >= 2);
        let joined: String = wrapped
            .iter()
            .flat_map(|row| row.iter())
            .map(|s| s.text.as_str())
            .collect();
        assert!(joined.contains("mundoñ"));
        assert!(joined.contains("bonito"));
    }

    #[test]
    fn rejects_remote_and_parent_escape_images() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("docs");
        std::fs::create_dir(&sub).unwrap();
        let doc = sub.join("README.md");
        std::fs::write(&doc, "# Test").unwrap();
        let img = sub.join("ok.png");
        image::RgbaImage::new(4, 4).save(&img).unwrap();
        let outside = dir.path().join("outside.png");
        image::RgbaImage::new(4, 4).save(&outside).unwrap();

        assert!(resolve_local_image(&doc, Some(&sub), "https://example.com/a.png").is_none());
        assert!(resolve_local_image(&doc, Some(&sub), "../outside.png").is_none());
        assert!(resolve_local_image(&doc, Some(&sub), "ok.png").is_some());
    }

    #[test]
    fn selection_and_search_cover_blocks() {
        let registry = ExtensionRegistry::new();
        let doc = RichDocument::from_markdown("# Título\n\nPárrafo uno\n\nPárrafo dos", &registry);
        let mut state = DocViewState {
            document: Some(doc),
            ..Default::default()
        };
        assert_eq!(state.find("Párrafo", true, false), "1 de 2");
        assert_eq!(state.find("Párrafo", true, false), "2 de 2");
        state.select_block(1, false);
        state.select_block(2, true);
        assert_eq!(state.selected_text(), "Párrafo uno\n\nPárrafo dos");
    }

    fn layout(width: f32) -> DocLayout {
        DocLayout {
            width,
            font: 15.0,
            zoom: 1.0,
        }
    }

    #[test]
    fn table_columns_share_one_width_and_keep_alignment() {
        let registry = ExtensionRegistry::new();
        let md = "| Año | Descripción | Total |\n|:---|:---:|---:|\n| 2024 | Canción del niño con una celda bastante larga | 1 |\n| 2025 | Peña | 12345 |\n";
        let state = DocViewState {
            document: Some(RichDocument::from_markdown(md, &registry)),
            ..Default::default()
        };
        let models = state.prepare_models(layout(1100.0));
        let table = &models.blocks[0];
        assert_eq!(table.kind, 5);
        assert_eq!(table.table_headers, ["Año", "Descripción", "Total"]);
        assert_eq!(table.col_widths.len(), 3);
        assert_eq!(table.col_aligns, [0, 1, 2]);
        // The widest cell of each column decides the width for every row.
        assert!(table.col_widths[1] > table.col_widths[0]);
        assert!(table.col_widths[1] > table.col_widths[2]);
        assert_eq!(table.table_rows[1][1], "Peña");
    }

    #[test]
    fn styled_paragraphs_wrap_to_the_real_column_width() {
        let registry = ExtensionRegistry::new();
        let md = "Texto con **negrita** y `código` que se repite varias veces para ocupar bastante más de una línea completa en la columna de lectura del documento.";
        let state = DocViewState {
            document: Some(RichDocument::from_markdown(md, &registry)),
            ..Default::default()
        };
        let wide = state.prepare_models(layout(1100.0)).blocks[0].lines.len();
        let narrow = state.prepare_models(layout(420.0)).blocks[0].lines.len();
        let zoomed = state
            .prepare_models(DocLayout {
                width: 420.0,
                font: 24.0,
                zoom: 1.6,
            })
            .blocks[0]
            .lines
            .len();
        assert!(wide >= 2, "wide rows: {wide}");
        assert!(narrow > wide, "narrow {narrow} vs wide {wide}");
        assert!(zoomed > narrow, "zoomed {zoomed} vs narrow {narrow}");
    }

    #[test]
    fn long_code_blocks_are_capped_until_expanded_and_selection_only_flags_rows() {
        let registry = ExtensionRegistry::new();
        let code: String = (0..120).map(|i| format!("linea {i}\n")).collect();
        let md = format!("Intro\n\n```text\n{code}```\n");
        let mut state = DocViewState {
            document: Some(RichDocument::from_markdown(&md, &registry)),
            ..Default::default()
        };
        let collapsed = state.prepare_models(layout(900.0));
        assert_eq!(collapsed.blocks[1].lines.len(), 40);
        assert_eq!(collapsed.blocks[1].more_lines, 80);
        assert!(collapsed.blocks[1].plain_text.contains("linea 119"));

        state.dirty = false;
        state.select_block(1, false);
        assert!(state.flags_dirty && !state.dirty);
        assert_eq!(state.window_flags(), [(false, false), (true, false)]);

        state.toggle_code_block(1);
        assert!(state.dirty);
        let expanded = state.prepare_models(layout(900.0));
        assert_eq!(expanded.blocks[1].lines.len(), 120);
        assert!(expanded.blocks[1].expanded);
    }
}
