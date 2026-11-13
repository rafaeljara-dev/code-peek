use std::collections::HashMap;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::languages::registry::ExtensionRegistry;
use crate::languages::traits::HighlightSpan;
use crate::themes::ColorRgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl From<BlockQuoteKind> for CalloutKind {
    fn from(kind: BlockQuoteKind) -> Self {
        match kind {
            BlockQuoteKind::Note => Self::Note,
            BlockQuoteKind::Tip => Self::Tip,
            BlockQuoteKind::Important => Self::Important,
            BlockQuoteKind::Warning => Self::Warning,
            BlockQuoteKind::Caution => Self::Caution,
        }
    }
}

impl CalloutKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Nota",
            Self::Tip => "Consejo",
            Self::Important => "Importante",
            Self::Warning => "Advertencia",
            Self::Caution => "Precaución",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlign {
    None,
    Left,
    Center,
    Right,
}

impl From<Alignment> for TableAlign {
    fn from(align: Alignment) -> Self {
        match align {
            Alignment::None => Self::None,
            Alignment::Left => Self::Left,
            Alignment::Center => Self::Center,
            Alignment::Right => Self::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichSpan {
    pub text: String,
    pub color: Option<ColorRgb>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub link: Option<String>,
}

impl RichSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: false,
            italic: false,
            strikethrough: false,
            code: false,
            link: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocHeading {
    pub level: u8,
    pub title: String,
    pub block_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichBlock {
    Heading {
        level: u8,
        spans: Vec<RichSpan>,
    },
    Paragraph {
        spans: Vec<RichSpan>,
    },
    Blockquote {
        callout: Option<CalloutKind>,
        paragraphs: Vec<Vec<RichSpan>>,
    },
    ListItem {
        depth: u8,
        ordered_index: Option<u64>,
        checked: Option<bool>,
        spans: Vec<RichSpan>,
    },
    CodeBlock {
        lang: Option<String>,
        lines: Vec<Vec<HighlightSpan>>,
        raw_code: String,
    },
    Table {
        alignments: Vec<TableAlign>,
        headers: Vec<Vec<RichSpan>>,
        rows: Vec<Vec<Vec<RichSpan>>>,
    },
    Image {
        alt: String,
        src: String,
    },
    ThematicBreak,
}

impl RichBlock {
    pub fn plain_text(&self) -> String {
        match self {
            Self::Heading { spans, .. }
            | Self::Paragraph { spans }
            | Self::ListItem { spans, .. } => spans_plain(spans),
            Self::Blockquote { paragraphs, .. } => paragraphs
                .iter()
                .map(|p| spans_plain(p))
                .collect::<Vec<_>>()
                .join("\n"),
            Self::CodeBlock { raw_code, .. } => raw_code.clone(),
            Self::Table { headers, rows, .. } => {
                let mut lines = Vec::with_capacity(rows.len() + 1);
                if !headers.is_empty() {
                    lines.push(
                        headers
                            .iter()
                            .map(|cell| spans_plain(cell))
                            .collect::<Vec<_>>()
                            .join(" | "),
                    );
                }
                for row in rows {
                    lines.push(
                        row.iter()
                            .map(|cell| spans_plain(cell))
                            .collect::<Vec<_>>()
                            .join(" | "),
                    );
                }
                lines.join("\n")
            }
            Self::Image { alt, src } => {
                if alt.is_empty() {
                    src.clone()
                } else {
                    format!("{alt} ({src})")
                }
            }
            Self::ThematicBreak => "---".into(),
        }
    }

    pub fn links(&self) -> Vec<String> {
        let mut out = Vec::new();
        let collect = |spans: &[RichSpan], acc: &mut Vec<String>| {
            for span in spans {
                if let Some(url) = &span.link {
                    if !acc.contains(url) {
                        acc.push(url.clone());
                    }
                }
            }
        };
        match self {
            Self::Heading { spans, .. }
            | Self::Paragraph { spans }
            | Self::ListItem { spans, .. } => collect(spans, &mut out),
            Self::Blockquote { paragraphs, .. } => {
                for p in paragraphs {
                    collect(p, &mut out);
                }
            }
            Self::Table { headers, rows, .. } => {
                for h in headers {
                    collect(h, &mut out);
                }
                for r in rows {
                    for c in r {
                        collect(c, &mut out);
                    }
                }
            }
            Self::CodeBlock { .. } | Self::Image { .. } | Self::ThematicBreak => {}
        }
        out
    }
}

pub(crate) fn spans_plain(spans: &[RichSpan]) -> String {
    let mut out = String::new();
    for s in spans {
        out.push_str(&s.text);
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RichDocument {
    pub blocks: Vec<RichBlock>,
    pub headings: Vec<DocHeading>,
    pub embedded_images: HashMap<String, Vec<u8>>,
}

impl RichDocument {
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(RichBlock::plain_text)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn from_markdown(source: &str, registry: &ExtensionRegistry) -> Self {
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_GFM;
        let parser = Parser::new_ext(source, options);

        let mut doc = Self::default();
        let mut style = InlineStyle::default();
        let mut active_spans: Vec<RichSpan> = Vec::new();

        let mut heading_level: Option<u8> = None;
        let mut in_paragraph = false;
        let mut in_code_block = false;
        let mut code_lang: Option<String> = None;
        let mut code_buf = String::new();

        let mut quote_depth = 0usize;
        let mut quote_callout: Option<CalloutKind> = None;
        let mut quote_paragraphs: Vec<Vec<RichSpan>> = Vec::new();

        let mut list_stack: Vec<Option<u64>> = Vec::new();
        let mut in_list_item = false;
        let mut current_item_ordered: Option<u64> = None;
        let mut current_item_checked: Option<bool> = None;
        let mut item_spans: Vec<RichSpan> = Vec::new();

        let mut in_table = false;
        let mut table_alignments: Vec<TableAlign> = Vec::new();
        let mut table_headers: Vec<Vec<RichSpan>> = Vec::new();
        let mut table_rows: Vec<Vec<Vec<RichSpan>>> = Vec::new();
        let mut current_row: Vec<Vec<RichSpan>> = Vec::new();
        let mut in_table_head = false;
        let mut in_table_cell = false;

        let mut in_image = false;
        let mut image_src = String::new();
        let mut image_alt = String::new();
        let mut pending_images: Vec<RichBlock> = Vec::new();

        let flush_item = |doc: &mut Self,
                          depth: u8,
                          ordered: Option<u64>,
                          checked: Option<bool>,
                          spans: &mut Vec<RichSpan>| {
            if !spans.is_empty() || checked.is_some() {
                doc.blocks.push(RichBlock::ListItem {
                    depth,
                    ordered_index: ordered,
                    checked,
                    spans: std::mem::take(spans),
                });
            }
        };

        for event in parser {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Heading { level, .. } => {
                        heading_level = Some(heading_to_u8(level));
                        active_spans.clear();
                    }
                    Tag::Paragraph => {
                        in_paragraph = true;
                        if quote_depth == 0 && !in_list_item && !in_table_cell {
                            active_spans.clear();
                        }
                    }
                    Tag::BlockQuote(kind) => {
                        if quote_depth == 0 {
                            quote_callout = kind.map(CalloutKind::from);
                            quote_paragraphs.clear();
                        }
                        quote_depth += 1;
                    }
                    Tag::CodeBlock(kind) => {
                        if in_list_item {
                            let depth = list_stack.len().saturating_sub(1) as u8;
                            flush_item(
                                &mut doc,
                                depth,
                                current_item_ordered,
                                current_item_checked,
                                &mut item_spans,
                            );
                            in_list_item = false;
                        }
                        in_code_block = true;
                        code_lang = match kind {
                            CodeBlockKind::Indented => None,
                            CodeBlockKind::Fenced(lang) => {
                                let trimmed = lang.split_whitespace().next().unwrap_or("").trim();
                                (!trimmed.is_empty()).then(|| trimmed.to_owned())
                            }
                        };
                        code_buf.clear();
                    }
                    Tag::List(start) => {
                        if in_list_item && !item_spans.is_empty() {
                            let depth = list_stack.len().saturating_sub(1) as u8;
                            flush_item(
                                &mut doc,
                                depth,
                                current_item_ordered,
                                current_item_checked,
                                &mut item_spans,
                            );
                            in_list_item = false;
                        }
                        list_stack.push(start);
                    }
                    Tag::Item => {
                        in_list_item = true;
                        current_item_checked = None;
                        item_spans.clear();
                        current_item_ordered = match list_stack.last_mut() {
                            Some(Some(next)) => {
                                let value = *next;
                                *next += 1;
                                Some(value)
                            }
                            _ => None,
                        };
                    }
                    Tag::Table(aligns) => {
                        in_table = true;
                        table_alignments = aligns.into_iter().map(TableAlign::from).collect();
                        table_headers.clear();
                        table_rows.clear();
                    }
                    Tag::TableHead => {
                        in_table_head = true;
                        current_row.clear();
                    }
                    Tag::TableRow => {
                        current_row.clear();
                    }
                    Tag::TableCell => {
                        in_table_cell = true;
                        active_spans.clear();
                    }
                    Tag::Strong => style.bold += 1,
                    Tag::Emphasis => style.italic += 1,
                    Tag::Strikethrough => style.strikethrough += 1,
                    Tag::Link { dest_url, .. } => style.link = Some(dest_url.into_string()),
                    Tag::Image { dest_url, .. } => {
                        in_image = true;
                        image_src = dest_url.into_string();
                        image_alt.clear();
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::Heading(_) => {
                        if let Some(level) = heading_level.take() {
                            let spans = std::mem::take(&mut active_spans);
                            let title = spans_plain(&spans);
                            let block_index = doc.blocks.len();
                            if !title.is_empty() {
                                doc.headings.push(DocHeading {
                                    level,
                                    title,
                                    block_index,
                                });
                            }
                            doc.blocks.push(RichBlock::Heading { level, spans });
                        }
                    }
                    TagEnd::Paragraph => {
                        in_paragraph = false;
                        if quote_depth > 0 {
                            let spans = std::mem::take(&mut active_spans);
                            if !spans.is_empty() {
                                quote_paragraphs.push(spans);
                            }
                        } else if in_list_item {
                            // Item spans accumulate across paragraph inside tight/loose item
                        } else if !in_table_cell {
                            let spans = std::mem::take(&mut active_spans);
                            if !spans.is_empty() {
                                doc.blocks.push(RichBlock::Paragraph { spans });
                            }
                            doc.blocks.append(&mut pending_images);
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        quote_depth = quote_depth.saturating_sub(1);
                        if quote_depth == 0 {
                            let paragraphs = std::mem::take(&mut quote_paragraphs);
                            let callout = quote_callout.take();
                            if !paragraphs.is_empty() {
                                doc.blocks.push(RichBlock::Blockquote {
                                    callout,
                                    paragraphs,
                                });
                            }
                        }
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        let lang = code_lang.take();
                        let raw_code = std::mem::take(&mut code_buf);
                        let lines = registry.highlight_code_snippet(lang.as_deref(), &raw_code);
                        doc.blocks.push(RichBlock::CodeBlock {
                            lang,
                            lines,
                            raw_code,
                        });
                    }
                    TagEnd::List(_) => {
                        list_stack.pop();
                    }
                    TagEnd::Item => {
                        if in_list_item {
                            let depth = list_stack.len().saturating_sub(1) as u8;
                            flush_item(
                                &mut doc,
                                depth,
                                current_item_ordered,
                                current_item_checked,
                                &mut item_spans,
                            );
                            in_list_item = false;
                        }
                        doc.blocks.append(&mut pending_images);
                    }
                    TagEnd::TableCell => {
                        in_table_cell = false;
                        current_row.push(std::mem::take(&mut active_spans));
                    }
                    TagEnd::TableHead => {
                        in_table_head = false;
                        table_headers = std::mem::take(&mut current_row);
                    }
                    TagEnd::TableRow => {
                        if !in_table_head {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    TagEnd::Table => {
                        in_table = false;
                        doc.blocks.push(RichBlock::Table {
                            alignments: std::mem::take(&mut table_alignments),
                            headers: std::mem::take(&mut table_headers),
                            rows: std::mem::take(&mut table_rows),
                        });
                    }
                    TagEnd::Strong => style.bold = style.bold.saturating_sub(1),
                    TagEnd::Emphasis => style.italic = style.italic.saturating_sub(1),
                    TagEnd::Strikethrough => {
                        style.strikethrough = style.strikethrough.saturating_sub(1)
                    }
                    TagEnd::Link => style.link = None,
                    TagEnd::Image => {
                        in_image = false;
                        let src = std::mem::take(&mut image_src);
                        let alt = std::mem::take(&mut image_alt);
                        if !src.is_empty() {
                            if in_paragraph || in_list_item {
                                pending_images.push(RichBlock::Image { alt, src });
                            } else {
                                doc.blocks.push(RichBlock::Image { alt, src });
                            }
                        }
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        code_buf.push_str(&text);
                    } else if in_image {
                        image_alt.push_str(&text);
                    } else {
                        let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                            &mut item_spans
                        } else {
                            &mut active_spans
                        };
                        push_styled_span(target, text.as_ref(), &style, false);
                    }
                }
                Event::Code(code) => {
                    if in_image {
                        image_alt.push_str(&code);
                    } else {
                        let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                            &mut item_spans
                        } else {
                            &mut active_spans
                        };
                        push_styled_span(target, code.as_ref(), &style, true);
                    }
                }
                Event::SoftBreak => {
                    if !in_code_block && !in_image {
                        let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                            &mut item_spans
                        } else {
                            &mut active_spans
                        };
                        push_styled_span(target, " ", &style, false);
                    }
                }
                Event::HardBreak => {
                    if !in_code_block && !in_image {
                        let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                            &mut item_spans
                        } else {
                            &mut active_spans
                        };
                        push_styled_span(target, "\n", &style, false);
                    }
                }
                Event::Rule => {
                    doc.blocks.push(RichBlock::ThematicBreak);
                }
                Event::TaskListMarker(checked) => {
                    if in_list_item {
                        current_item_checked = Some(checked);
                    }
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    let trimmed = html.trim();
                    if !trimmed.is_empty()
                        && !trimmed.starts_with("<!--")
                        && (in_paragraph || in_list_item || quote_depth > 0 || in_table_cell)
                    {
                        let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                            &mut item_spans
                        } else {
                            &mut active_spans
                        };
                        push_styled_span(target, trimmed, &style, true);
                    }
                }
                Event::InlineMath(math) | Event::DisplayMath(math) => {
                    let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                        &mut item_spans
                    } else {
                        &mut active_spans
                    };
                    push_styled_span(target, math.as_ref(), &style, true);
                }
                Event::FootnoteReference(label) => {
                    let target = if in_list_item && quote_depth == 0 && !in_table_cell {
                        &mut item_spans
                    } else {
                        &mut active_spans
                    };
                    push_styled_span(target, &format!("[^{label}]"), &style, false);
                }
            }
        }

        if in_table && (!table_headers.is_empty() || !table_rows.is_empty()) {
            doc.blocks.push(RichBlock::Table {
                alignments: table_alignments,
                headers: table_headers,
                rows: table_rows,
            });
        }
        doc.blocks.extend(pending_images);
        doc
    }
}

#[derive(Default)]
struct InlineStyle {
    bold: usize,
    italic: usize,
    strikethrough: usize,
    link: Option<String>,
}

fn push_styled_span(spans: &mut Vec<RichSpan>, text: &str, style: &InlineStyle, code: bool) {
    if text.is_empty() {
        return;
    }
    let bold = style.bold > 0;
    let italic = style.italic > 0;
    let strikethrough = style.strikethrough > 0;
    let link = style.link.clone();
    if let Some(last) = spans.last_mut().filter(|last| {
        last.bold == bold
            && last.italic == italic
            && last.strikethrough == strikethrough
            && last.code == code
            && last.link == link
            && last.color.is_none()
    }) {
        last.text.push_str(text);
    } else {
        spans.push(RichSpan {
            text: text.to_owned(),
            color: None,
            bold,
            italic,
            strikethrough,
            code,
            link,
        });
    }
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gfm_headings_tables_code_callouts_tasks_and_images() {
        let md = "\
# Guía principal

Texto **en negrita**, *cursiva*, ~~tachado~~, `inline` y [enlace](https://example.com).

> [!NOTE]
> Bloque de aviso importante.

- [x] Tarea hecha
- [ ] Pendiente

| Nombre | Estado |
| --- | :---: |
| Rust | OK |

```rust
fn hello() -> u32 { 42 }
```

![Logo](assets/icon.png)

---
";
        let registry = ExtensionRegistry::new();
        let doc = RichDocument::from_markdown(md, &registry);
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].title, "Guía principal");
        assert!(matches!(doc.blocks[0], RichBlock::Heading { level: 1, .. }));
        assert!(matches!(doc.blocks[1], RichBlock::Paragraph { .. }));
        assert!(matches!(
            doc.blocks[2],
            RichBlock::Blockquote {
                callout: Some(CalloutKind::Note),
                ..
            }
        ));
        assert!(matches!(
            doc.blocks[3],
            RichBlock::ListItem {
                checked: Some(true),
                ..
            }
        ));
        assert!(matches!(
            doc.blocks[4],
            RichBlock::ListItem {
                checked: Some(false),
                ..
            }
        ));
        if let RichBlock::Table {
            alignments,
            headers,
            rows,
        } = &doc.blocks[5]
        {
            assert_eq!(alignments, &[TableAlign::None, TableAlign::Center]);
            assert_eq!(headers.len(), 2);
            assert_eq!(rows.len(), 1);
        } else {
            panic!("Se esperaba una tabla GFM");
        }
        if let RichBlock::CodeBlock { lang, lines, .. } = &doc.blocks[6] {
            assert_eq!(lang.as_deref(), Some("rust"));
            assert_eq!(lines.len(), 1);
            assert!(lines[0].len() > 1);
        } else {
            panic!("Se esperaba bloque de código");
        }
        assert!(matches!(
            &doc.blocks[7],
            RichBlock::Image { alt, src } if alt == "Logo" && src == "assets/icon.png"
        ));
        assert!(matches!(doc.blocks[8], RichBlock::ThematicBreak));
    }
}
