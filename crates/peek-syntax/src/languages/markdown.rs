use super::traits::{HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType};
use crate::themes::ColorRgb;

pub struct MarkdownExtension;

impl LanguageExtension for MarkdownExtension {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn name(&self) -> &'static str {
        "Markdown"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["md", "markdown", "mdown", "mkdn"]
    }

    fn is_markdown(&self) -> bool {
        true
    }

    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        ctx: &mut LineContext,
    ) -> HighlightedLine {
        let trimmed = line.trim();

        // 1. Code fence delimiter: ```lang
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            ctx.in_code_block = !ctx.in_code_block;
            let fence_tag = trimmed.trim_start_matches(['`', '~']).trim();
            if ctx.in_code_block && !fence_tag.is_empty() {
                ctx.code_block_lang = Some(fence_tag.to_string());
            } else if !ctx.in_code_block {
                ctx.code_block_lang = None;
            }

            return HighlightedLine {
                line_number,
                line_type: LineType::CodeFence,
                spans: vec![
                    HighlightSpan::new("```", ColorRgb::new(100, 116, 139)), // slate-500
                    HighlightSpan::new(format!(" {}", fence_tag), ColorRgb::new(251, 191, 36)), // amber-400
                ],
                is_markdown: true,
            };
        }

        // 2. Inside code block
        if ctx.in_code_block {
            return HighlightedLine {
                line_number,
                line_type: LineType::CodeFence,
                spans: vec![HighlightSpan::new(line, ColorRgb::new(226, 232, 240))], // slate-200
                is_markdown: true,
            };
        }

        // 3. Horizontal Rule: ---, ***, ___
        if (trimmed == "---" || trimmed == "***" || trimmed == "___") && trimmed.len() >= 3 {
            return HighlightedLine {
                line_number,
                line_type: LineType::Divider,
                spans: vec![HighlightSpan::new(
                    "────────────────────────────────────────────────────────",
                    ColorRgb::new(63, 63, 70),
                )],
                is_markdown: true,
            };
        }

        // 4. Headings
        if trimmed.starts_with('#') {
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if hash_count <= 6 && trimmed[hash_count..].starts_with(' ') {
                let (line_type, hash_color, text_color) = match hash_count {
                    1 => (
                        LineType::Heading1,
                        ColorRgb::new(56, 189, 248),
                        ColorRgb::new(248, 250, 252),
                    ), // sky-400, white
                    2 => (
                        LineType::Heading2,
                        ColorRgb::new(129, 140, 248),
                        ColorRgb::new(241, 245, 249),
                    ), // indigo-400
                    _ => (
                        LineType::Heading3,
                        ColorRgb::new(167, 139, 250),
                        ColorRgb::new(226, 232, 240),
                    ), // violet-400
                };

                let prefix = &trimmed[0..hash_count];
                let title = &trimmed[hash_count..];

                return HighlightedLine {
                    line_number,
                    line_type,
                    spans: vec![
                        HighlightSpan::bold(prefix, hash_color),
                        HighlightSpan::bold(title, text_color),
                    ],
                    is_markdown: true,
                };
            }
        }

        // 5. Blockquotes: > text
        if let Some(stripped) = trimmed.strip_prefix('>') {
            let quote_text = stripped.trim();
            return HighlightedLine {
                line_number,
                line_type: LineType::Blockquote,
                spans: vec![
                    HighlightSpan::new("▎ ", ColorRgb::new(100, 116, 139)), // slate-500 quote bar
                    HighlightSpan::italic(quote_text, ColorRgb::new(203, 213, 225)), // slate-300
                ],
                is_markdown: true,
            };
        }

        // 6. Lists and task checkboxes: - [ ] or - [x] or * or 1.
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let indent = line.len() - line.trim_start().len();
            let indent_str = " ".repeat(indent);
            let bullet_char = &trimmed[0..2];
            let rest = &trimmed[2..];

            let mut spans = Vec::new();
            if !indent_str.is_empty() {
                spans.push(HighlightSpan::new(indent_str, ColorRgb::new(113, 113, 122)));
            }

            // Checkbox: [ ] or [x]
            if let Some(after_box) = rest.strip_prefix("[ ] ") {
                spans.push(HighlightSpan::bold("• ", ColorRgb::new(245, 158, 11)));
                spans.push(HighlightSpan::new("[ ] ", ColorRgb::new(113, 113, 122)));
                tokenize_inline_markdown(after_box, &mut spans);
            } else if let Some(after_box) = rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                spans.push(HighlightSpan::bold("• ", ColorRgb::new(16, 185, 129)));
                spans.push(HighlightSpan::bold("[✓] ", ColorRgb::new(16, 185, 129))); // emerald checked
                tokenize_inline_markdown(after_box, &mut spans);
            } else {
                spans.push(HighlightSpan::bold(
                    bullet_char,
                    ColorRgb::new(245, 158, 11),
                )); // amber bullet
                tokenize_inline_markdown(rest, &mut spans);
            }

            return HighlightedLine {
                line_number,
                line_type: LineType::ListItem,
                spans,
                is_markdown: true,
            };
        }

        // 7. General text with inline formatting
        let mut spans = Vec::new();
        tokenize_inline_markdown(line, &mut spans);

        HighlightedLine {
            line_number,
            line_type: LineType::Normal,
            spans,
            is_markdown: true,
        }
    }
}

/// Tokenize inline Markdown patterns: `code`, **bold**, *italic*, [link](url)
fn tokenize_inline_markdown(text: &str, spans: &mut Vec<HighlightSpan>) {
    if text.is_empty() {
        return;
    }

    let mut remaining = text;
    let text_color = ColorRgb::new(228, 228, 231); // zinc-200
    let code_color = ColorRgb::new(56, 189, 248); // sky-400
    let bold_color = ColorRgb::new(244, 63, 94); // rose-500
    let italic_color = ColorRgb::new(251, 191, 36); // amber-400
    let link_color = ColorRgb::new(59, 130, 246); // blue-500
    let muted_color = ColorRgb::new(113, 113, 122); // zinc-500

    while !remaining.is_empty() {
        // Inline code `code`
        if let Some(start) = remaining.find('`') {
            if let Some(end) = remaining[start + 1..].find('`') {
                let actual_end = start + 1 + end;
                if start > 0 {
                    spans.push(HighlightSpan::new(&remaining[..start], text_color));
                }
                let code_content = &remaining[start..=actual_end];
                spans.push(HighlightSpan::new(code_content, code_color));
                remaining = &remaining[actual_end + 1..];
                continue;
            }
        }

        // Bold **word**
        if let Some(start) = remaining.find("**") {
            if let Some(end) = remaining[start + 2..].find("**") {
                let actual_end = start + 2 + end;
                if start > 0 {
                    spans.push(HighlightSpan::new(&remaining[..start], text_color));
                }
                let bold_content = &remaining[start + 2..actual_end];
                spans.push(HighlightSpan::bold(bold_content, bold_color));
                remaining = &remaining[actual_end + 2..];
                continue;
            }
        }

        // Links [text](url)
        if let Some(start) = remaining.find('[') {
            if let Some(bracket_end) = remaining[start + 1..].find(']') {
                let actual_bracket_end = start + 1 + bracket_end;
                if remaining[actual_bracket_end + 1..].starts_with('(') {
                    if let Some(paren_end) = remaining[actual_bracket_end + 2..].find(')') {
                        let actual_paren_end = actual_bracket_end + 2 + paren_end;
                        if start > 0 {
                            spans.push(HighlightSpan::new(&remaining[..start], text_color));
                        }
                        let link_label = &remaining[start + 1..actual_bracket_end];
                        let link_url = &remaining[actual_bracket_end + 2..actual_paren_end];
                        spans.push(HighlightSpan::bold(format!("[{}]", link_label), link_color));
                        spans.push(HighlightSpan::new(format!("({})", link_url), muted_color));
                        remaining = &remaining[actual_paren_end + 1..];
                        continue;
                    }
                }
            }
        }

        // Italic *word*
        if let Some(start) = remaining.find('*') {
            if let Some(end) = remaining[start + 1..].find('*') {
                let actual_end = start + 1 + end;
                if start > 0 {
                    spans.push(HighlightSpan::new(&remaining[..start], text_color));
                }
                let italic_content = &remaining[start + 1..actual_end];
                spans.push(HighlightSpan::italic(italic_content, italic_color));
                remaining = &remaining[actual_end + 1..];
                continue;
            }
        }

        // No more special inline markers
        spans.push(HighlightSpan::new(remaining, text_color));
        break;
    }
}
