use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::Path;

use anyhow::{bail, Context, Result};
use roxmltree::{Document, Node, ParsingOptions};
use zip::ZipArchive;

use crate::rich_doc::{spans_plain, DocHeading, RichBlock, RichDocument, RichSpan, TableAlign};

const MAX_DOCX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XML_UNCOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_SINGLE_MEDIA_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_MEDIA_BYTES: usize = 32 * 1024 * 1024;
const MAX_MEDIA_COUNT: usize = 32;
const MAX_XML_NODES: u32 = 500_000;

pub fn is_docx_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
}

pub fn load_docx_file(path: &Path) -> Result<RichDocument> {
    let meta = fs::metadata(path).with_context(|| format!("No se pudo leer {}", path.display()))?;
    if meta.len() > MAX_DOCX_FILE_BYTES {
        bail!(
            "El documento Word supera el límite seguro de {} MB",
            MAX_DOCX_FILE_BYTES / (1024 * 1024)
        );
    }
    let file = File::open(path).with_context(|| format!("No se pudo abrir {}", path.display()))?;
    load_docx_reader(file)
}

pub fn load_docx_reader<R: Read + Seek>(reader: R) -> Result<RichDocument> {
    let mut archive = ZipArchive::new(reader).context("El archivo .docx no es un ZIP válido")?;
    let rels_xml = read_zip_text_optional(&mut archive, "word/_rels/document.xml.rels")?;
    let rels = rels_xml
        .as_deref()
        .map(parse_relationships)
        .transpose()?
        .unwrap_or_default();

    let document_xml = read_zip_text_required(&mut archive, "word/document.xml")?;
    let xml = parse_xml(&document_xml)?;

    let mut doc = RichDocument::default();
    let mut media_bytes_total = 0usize;

    let Some(body) = find_child_by_name(xml.root_element(), "body") else {
        bail!("El archivo .docx no contiene un cuerpo word/document.xml válido");
    };

    for child in body.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "p" => parse_paragraph(child, &rels, &mut archive, &mut doc, &mut media_bytes_total)?,
            "tbl" => {
                if let Some(table) = parse_table(child, &rels) {
                    doc.blocks.push(table);
                }
            }
            "sdt" => {
                // Structured document tags can wrap headings/paragraphs in Word templates.
                if let Some(content) = find_child_by_name(child, "sdtContent") {
                    for inner in content.children().filter(Node::is_element) {
                        if inner.tag_name().name() == "p" {
                            parse_paragraph(
                                inner,
                                &rels,
                                &mut archive,
                                &mut doc,
                                &mut media_bytes_total,
                            )?;
                        } else if inner.tag_name().name() == "tbl" {
                            if let Some(table) = parse_table(inner, &rels) {
                                doc.blocks.push(table);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(doc)
}

#[derive(Default)]
struct DocxRelationships {
    external_links: HashMap<String, String>,
    image_targets: HashMap<String, String>,
}

fn parse_relationships(xml_text: &str) -> Result<DocxRelationships> {
    let doc = parse_xml(xml_text)?;
    let mut rels = DocxRelationships::default();
    for node in doc.root_element().children().filter(Node::is_element) {
        if node.tag_name().name() != "Relationship" {
            continue;
        }
        let Some(id) = node.attribute("Id") else {
            continue;
        };
        let Some(target) = node.attribute("Target") else {
            continue;
        };
        let target_mode = node.attribute("TargetMode").unwrap_or_default();
        let rel_type = node.attribute("Type").unwrap_or_default();
        if target_mode.eq_ignore_ascii_case("External") {
            if target.starts_with("https://") || target.starts_with("http://") {
                rels.external_links.insert(id.to_owned(), target.to_owned());
            }
        } else if rel_type.ends_with("/image") {
            if let Some(entry_path) = normalize_word_target(target) {
                rels.image_targets.insert(id.to_owned(), entry_path);
            }
        }
    }
    Ok(rels)
}

fn normalize_word_target(target: &str) -> Option<String> {
    let normalized = target.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let resolved = if trimmed.starts_with("word/") {
        trimmed.to_owned()
    } else {
        format!("word/{trimmed}")
    };
    if resolved.contains("..") || resolved.contains('\0') || !resolved.starts_with("word/media/") {
        return None;
    }
    Some(resolved)
}

fn parse_paragraph<R: Read + Seek>(
    para: Node<'_, '_>,
    rels: &DocxRelationships,
    archive: &mut ZipArchive<R>,
    doc: &mut RichDocument,
    media_bytes_total: &mut usize,
) -> Result<()> {
    let mut heading_level: Option<u8> = None;
    let mut is_list = false;
    let mut is_quote = false;
    let mut is_thematic_break = false;

    if let Some(ppr) = find_child_by_name(para, "pPr") {
        if let Some(style) = find_child_by_name(ppr, "pStyle").and_then(|n| attr_val(n, "val")) {
            heading_level = classify_heading_style(style);
            let lower = style.to_ascii_lowercase();
            is_quote = lower.contains("quote") || lower.contains("cita");
        }
        if heading_level.is_none() {
            if let Some(lvl) = find_child_by_name(ppr, "outlineLvl")
                .and_then(|n| attr_val(n, "val"))
                .and_then(|v| v.parse::<u8>().ok())
            {
                if lvl < 6 {
                    heading_level = Some(lvl + 1);
                }
            }
        }
        is_list = find_child_by_name(ppr, "numPr").is_some();
        if let Some(pbdr) = find_child_by_name(ppr, "pBdr") {
            if find_child_by_name(pbdr, "bottom").is_some() {
                is_thematic_break = true;
            }
        }
    }

    let mut spans: Vec<RichSpan> = Vec::new();
    let mut image_ids: Vec<(String, String)> = Vec::new();
    collect_inline_nodes(para, rels, None, &mut spans, &mut image_ids);

    if let Some(level) = heading_level {
        let title = spans_plain(&spans).trim().to_owned();
        if !title.is_empty() {
            let block_index = doc.blocks.len();
            doc.headings.push(DocHeading {
                level,
                title,
                block_index,
            });
            doc.blocks.push(RichBlock::Heading { level, spans });
        }
    } else if is_list && !spans.is_empty() {
        doc.blocks.push(RichBlock::ListItem {
            depth: 0,
            ordered_index: None,
            checked: None,
            spans,
        });
    } else if is_quote && !spans.is_empty() {
        doc.blocks.push(RichBlock::Blockquote {
            callout: None,
            paragraphs: vec![spans],
        });
    } else if !spans.is_empty() {
        doc.blocks.push(RichBlock::Paragraph { spans });
    } else if is_thematic_break {
        doc.blocks.push(RichBlock::ThematicBreak);
    }

    for (rel_id, alt) in image_ids {
        let Some(entry_path) = rels.image_targets.get(&rel_id) else {
            continue;
        };
        if !doc.embedded_images.contains_key(entry_path)
            && doc.embedded_images.len() < MAX_MEDIA_COUNT
            && *media_bytes_total < MAX_TOTAL_MEDIA_BYTES
        {
            if let Ok(Some(bytes)) = read_zip_bytes_capped(
                archive,
                entry_path,
                MAX_SINGLE_MEDIA_BYTES.min(MAX_TOTAL_MEDIA_BYTES - *media_bytes_total),
            ) {
                *media_bytes_total += bytes.len();
                doc.embedded_images.insert(entry_path.clone(), bytes);
            }
        }
        if doc.embedded_images.contains_key(entry_path) {
            doc.blocks.push(RichBlock::Image {
                alt,
                src: entry_path.clone(),
            });
        }
    }

    Ok(())
}

fn parse_table(table: Node<'_, '_>, rels: &DocxRelationships) -> Option<RichBlock> {
    let mut rows_out: Vec<Vec<Vec<RichSpan>>> = Vec::new();
    for tr in table
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "tr")
    {
        let mut row_cells = Vec::new();
        for tc in tr
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "tc")
        {
            let mut cell_spans: Vec<RichSpan> = Vec::new();
            let mut unused_imgs = Vec::new();
            let mut first_para = true;
            for p in tc
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "p")
            {
                if !first_para && !cell_spans.is_empty() {
                    push_docx_span(
                        &mut cell_spans,
                        RichSpan {
                            text: " ".into(),
                            color: None,
                            bold: false,
                            italic: false,
                            strikethrough: false,
                            code: false,
                            link: None,
                        },
                    );
                }
                first_para = false;
                collect_inline_nodes(p, rels, None, &mut cell_spans, &mut unused_imgs);
            }
            row_cells.push(cell_spans);
        }
        if !row_cells.is_empty() {
            rows_out.push(row_cells);
        }
    }
    if rows_out.is_empty() {
        return None;
    }
    let headers = rows_out.remove(0);
    let alignments = vec![TableAlign::None; headers.len()];
    Some(RichBlock::Table {
        headers,
        alignments,
        rows: rows_out,
    })
}

fn collect_inline_nodes(
    container: Node<'_, '_>,
    rels: &DocxRelationships,
    active_link: Option<&str>,
    spans: &mut Vec<RichSpan>,
    image_ids: &mut Vec<(String, String)>,
) {
    for child in container.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "r" => parse_run(child, active_link, spans, image_ids),
            "hyperlink" => {
                let url = attr_val(child, "id").and_then(|id| rels.external_links.get(id));
                collect_inline_nodes(
                    child,
                    rels,
                    url.map(String::as_str).or(active_link),
                    spans,
                    image_ids,
                );
            }
            _ => {}
        }
    }
}

fn parse_run(
    run: Node<'_, '_>,
    active_link: Option<&str>,
    spans: &mut Vec<RichSpan>,
    image_ids: &mut Vec<(String, String)>,
) {
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut code = false;

    if let Some(rpr) = find_child_by_name(run, "rPr") {
        bold = is_flag_on(&rpr, "b");
        italic = is_flag_on(&rpr, "i");
        strikethrough = is_flag_on(&rpr, "strike") || is_flag_on(&rpr, "dstrike");
        if let Some(rfonts) = find_child_by_name(rpr, "rFonts") {
            let font_name = attr_val(rfonts, "ascii")
                .unwrap_or_default()
                .to_ascii_lowercase();
            code = font_name.contains("consolas")
                || font_name.contains("courier")
                || font_name.contains("mono")
                || font_name.contains("fira code");
        }
    }

    for node in run.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "t" | "delText" | "instrText" => {
                if let Some(text) = node.text() {
                    if !text.is_empty() {
                        push_docx_span(
                            spans,
                            RichSpan {
                                text: text.to_owned(),
                                color: None,
                                bold,
                                italic,
                                strikethrough,
                                code,
                                link: active_link.map(str::to_owned),
                            },
                        );
                    }
                }
            }
            "tab" => push_docx_span(
                spans,
                RichSpan {
                    text: "    ".into(),
                    color: None,
                    bold,
                    italic,
                    strikethrough,
                    code,
                    link: active_link.map(str::to_owned),
                },
            ),
            "br" => push_docx_span(
                spans,
                RichSpan {
                    text: "\n".into(),
                    color: None,
                    bold,
                    italic,
                    strikethrough,
                    code,
                    link: active_link.map(str::to_owned),
                },
            ),
            "drawing" | "pict" => {
                let mut alt = String::new();
                for desc in node.descendants().filter(Node::is_element) {
                    if desc.tag_name().name() == "docPr" {
                        if let Some(descr) = desc
                            .attribute("descr")
                            .or_else(|| desc.attribute("name"))
                            .filter(|s| !s.trim().is_empty())
                        {
                            alt = descr.trim().to_owned();
                        }
                    } else if desc.tag_name().name() == "blip" {
                        if let Some(embed_id) = attr_val(desc, "embed") {
                            image_ids.push((embed_id.to_owned(), std::mem::take(&mut alt)));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn push_docx_span(spans: &mut Vec<RichSpan>, item: RichSpan) {
    if item.text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut().filter(|last| {
        last.color == item.color
            && last.bold == item.bold
            && last.italic == item.italic
            && last.strikethrough == item.strikethrough
            && last.code == item.code
            && last.link == item.link
    }) {
        last.text.push_str(&item.text);
    } else {
        spans.push(item);
    }
}

fn classify_heading_style(style: &str) -> Option<u8> {
    let trimmed = style.trim();
    if trimmed.eq_ignore_ascii_case("Title") || trimmed.eq_ignore_ascii_case("Título") {
        return Some(1);
    }
    if trimmed.eq_ignore_ascii_case("Subtitle") || trimmed.eq_ignore_ascii_case("Subtítulo") {
        return Some(2);
    }
    for prefix in ["Heading", "heading", "Título", "Titulo", "título", "titulo"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            if let Ok(level) = rest.trim().parse::<u8>() {
                if (1..=6).contains(&level) {
                    return Some(level);
                }
            }
        }
    }
    None
}

fn is_flag_on(rpr: &Node<'_, '_>, tag: &str) -> bool {
    let Some(flag) = find_child_by_name(*rpr, tag) else {
        return false;
    };
    !matches!(attr_val(flag, "val"), Some("0" | "false" | "FALSE" | "off"))
}

fn find_child_by_name<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

fn attr_val<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attr| attr.name() == name)
        .map(|attr| attr.value())
}

fn parse_xml(text: &str) -> Result<Document<'_>> {
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_XML_NODES,
            ..Default::default()
        },
    )
    .context("XML interno de .docx inválido o demasiado complejo")
}

fn read_zip_text_required<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String> {
    read_zip_text_optional(archive, name)?
        .with_context(|| format!("Falta la entrada obligatoria {name} en el .docx"))
}

fn read_zip_text_optional<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<String>> {
    let Some(bytes) = read_zip_bytes_capped(archive, name, MAX_XML_UNCOMPRESSED_BYTES)? else {
        return Ok(None);
    };
    // OOXML allows UTF-16 parts; anything else must be strict UTF-8 (never guessed).
    let text = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        peek_core::decode_text_bytes(&bytes)
            .with_context(|| format!("La entrada {name} no es UTF-16 válido"))?
            .0
            .into_owned()
    } else {
        String::from_utf8(bytes).with_context(|| format!("La entrada {name} no es UTF-8 válido"))?
    };
    Ok(Some(text))
}

fn read_zip_bytes_capped<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if entry.size() > limit as u64 {
        bail!("La entrada {name} supera el límite seguro de tamaño");
    }
    let mut buf = Vec::with_capacity((entry.size() as usize).min(limit));
    entry
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut buf)?;
    if buf.len() > limit {
        bail!("La entrada {name} excedió el tamaño descomprimido permitido");
    }
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn build_sample_docx(doc_xml: &str, rels_xml: Option<&str>) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default();
            zip.start_file("word/document.xml", opts).unwrap();
            zip.write_all(doc_xml.as_bytes()).unwrap();
            if let Some(rels) = rels_xml {
                zip.start_file("word/_rels/document.xml.rels", opts)
                    .unwrap();
                zip.write_all(rels.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn parses_headings_formatted_paragraphs_links_lists_and_tables() {
        let doc_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Resumen ejecutivo</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t xml:space="preserve">Texto </w:t></w:r>
      <w:r><w:rPr><w:b/></w:rPr><w:t>importante</w:t></w:r>
      <w:hyperlink r:id="rId1">
        <w:r><w:t> enlace</w:t></w:r>
      </w:hyperlink>
    </w:p>
    <w:p>
      <w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>
      <w:r><w:t>Elemento de lista</w:t></w:r>
    </w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>Col A</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>Col B</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>10</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>20</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;
        let rels_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com" TargetMode="External"/>
</Relationships>"#;

        let bytes = build_sample_docx(doc_xml, Some(rels_xml));
        let doc = load_docx_reader(Cursor::new(bytes)).unwrap();
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].title, "Resumen ejecutivo");
        assert!(matches!(doc.blocks[0], RichBlock::Heading { level: 1, .. }));
        assert!(matches!(doc.blocks[1], RichBlock::Paragraph { .. }));
        assert!(doc.blocks[1]
            .links()
            .contains(&"https://example.com".to_owned()));
        assert!(matches!(doc.blocks[2], RichBlock::ListItem { .. }));
        assert!(matches!(doc.blocks[3], RichBlock::Table { .. }));
    }

    #[test]
    fn rejects_dtd_inside_docx_xml() {
        let bad_xml = r#"<?xml version="1.0"?><!DOCTYPE foo [<!ENTITY x "y">]><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        let bytes = build_sample_docx(bad_xml, None);
        assert!(load_docx_reader(Cursor::new(bytes)).is_err());
    }
}
