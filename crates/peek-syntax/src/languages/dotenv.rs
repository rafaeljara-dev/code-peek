use super::traits::{HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType};
use crate::themes::ColorRgb;

pub struct DotenvExtension;

const TEXT: ColorRgb = ColorRgb::new(212, 212, 216);
const KEY: ColorRgb = ColorRgb::new(156, 220, 254);
const OPERATOR: ColorRgb = ColorRgb::new(197, 134, 192);
const QUOTE: ColorRgb = ColorRgb::new(220, 220, 170);
const VALUE: ColorRgb = ColorRgb::new(206, 145, 120);
const COMMENT: ColorRgb = ColorRgb::new(106, 153, 85);

fn push(spans: &mut Vec<HighlightSpan>, text: &str, color: ColorRgb) {
    if !text.is_empty() {
        spans.push(HighlightSpan::new(text, color));
    }
}

impl LanguageExtension for DotenvExtension {
    fn id(&self) -> &'static str {
        "dotenv"
    }
    fn name(&self) -> &'static str {
        "Dotenv"
    }
    fn file_extensions(&self) -> &'static [&'static str] {
        &["env"]
    }

    fn matches_filename(&self, filename: &str) -> bool {
        let name = filename.to_ascii_lowercase();
        name == ".env" || name.starts_with(".env.")
    }

    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        ctx: &mut LineContext,
    ) -> HighlightedLine {
        let mut spans = Vec::new();
        let mut offset = 0;
        if ctx.dotenv_quote.is_none() {
            let trimmed = line.trim_start_matches([' ', '\t', '\u{feff}']);
            offset = line.len() - trimmed.len();
            push(&mut spans, &line[..offset], TEXT);
            if trimmed.starts_with('#') {
                push(&mut spans, trimmed, COMMENT);
                offset = line.len();
            } else {
                if trimmed
                    .strip_prefix("export")
                    .is_some_and(|rest| rest.starts_with([' ', '\t']))
                {
                    push(&mut spans, "export", OPERATOR);
                    offset += 6;
                    let rest = &line[offset..];
                    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                    push(&mut spans, &rest[..spaces], TEXT);
                    offset += spaces;
                }
                let rest = &line[offset..];
                if let Some(eq) = rest.find('=') {
                    let key = rest[..eq].trim_end_matches([' ', '\t']);
                    push(&mut spans, key, KEY);
                    push(&mut spans, &rest[key.len()..eq], TEXT);
                    push(&mut spans, "=", OPERATOR);
                    offset += eq + 1;
                    let rest = &line[offset..];
                    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                    push(&mut spans, &rest[..spaces], TEXT);
                    offset += spaces;
                    if let Some(quote @ ('\'' | '"' | '`')) = line[offset..].chars().next() {
                        push(&mut spans, &line[offset..offset + 1], QUOTE);
                        ctx.dotenv_quote = Some(quote);
                        offset += 1;
                    }
                } else {
                    // Incomplete assignments remain readable while typing.
                    let end = rest.find('#').unwrap_or(rest.len());
                    push(&mut spans, &rest[..end], KEY);
                    push(&mut spans, &rest[end..], COMMENT);
                    offset = line.len();
                }
            }
        }

        let mut start = offset;
        let mut escaped = false;
        for (relative, ch) in line[offset..].char_indices() {
            let index = offset + relative;
            if let Some(quote) = ctx.dotenv_quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == quote {
                    push(&mut spans, &line[start..index], VALUE);
                    push(&mut spans, &line[index..index + ch.len_utf8()], QUOTE);
                    start = index + ch.len_utf8();
                    ctx.dotenv_quote = None;
                }
            } else if ch == '#' {
                push(&mut spans, &line[start..index], VALUE);
                push(&mut spans, &line[index..], COMMENT);
                start = line.len();
                break;
            }
        }
        push(&mut spans, &line[start..], VALUE);
        HighlightedLine {
            line_number,
            line_type: LineType::Normal,
            spans,
            is_markdown: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExtensionRegistry;
    use peek_core::TextBuffer;
    use std::path::Path;

    #[test]
    fn filenames_take_priority_over_suffix_languages() {
        let registry = ExtensionRegistry::new();
        for name in [
            ".env",
            ".env.local",
            ".env.production.local",
            ".env.example",
            ".env.test",
            ".env.json",
            ".ENV.LOCAL",
            "config.env",
        ] {
            assert_eq!(registry.language_name(Path::new(name)), "Dotenv", "{name}");
        }
        for name in ["env.rs", ".environment", "local.json", ".envrc"] {
            assert_ne!(registry.language_name(Path::new(name)), "Dotenv", "{name}");
        }
    }

    #[test]
    fn assignments_have_distinct_key_operator_quote_and_value_colors() {
        for source in ["NAME=hola", "NAME=\"hola\"", "NAME='hola'", "NAME=`hola`"] {
            let line = DotenvExtension.tokenize_line(source, 1, &mut LineContext::default());
            assert!(line
                .spans
                .iter()
                .any(|s| s.text == "NAME" && s.color == KEY));
            assert!(line
                .spans
                .iter()
                .any(|s| s.text == "=" && s.color == OPERATOR));
            assert!(line
                .spans
                .iter()
                .any(|s| s.text == "hola" && s.color == VALUE));
            if source != "NAME=hola" {
                assert_eq!(line.spans.iter().filter(|s| s.color == QUOTE).count(), 2);
            }
        }
        assert_ne!(KEY, OPERATOR);
        assert_ne!(KEY, VALUE);
        assert_ne!(QUOTE, VALUE);
    }

    #[test]
    fn preserves_source_and_recognizes_only_unquoted_comments() {
        for (source, comment) in [
            ("  #--- configuración = \"x\"", "#--- configuración = \"x\""),
            ("export NAME = \"niño # valor\" # nota", "# nota"),
            ("TOKEN=abc==#nota", "#nota"),
            (r##"NAME="a\"#b"#nota"##, "#nota"),
            ("NAME='a#b'", ""),
            ("EMPTY=\"\"", ""),
            ("EMPTY=", ""),
            ("NAME", ""),
            ("  ", ""),
            ("", ""),
        ] {
            let line = DotenvExtension.tokenize_line(source, 1, &mut LineContext::default());
            assert_eq!(
                line.spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>(),
                source
            );
            assert_eq!(
                line.spans
                    .iter()
                    .filter(|s| s.color == COMMENT)
                    .map(|s| s.text.as_str())
                    .collect::<String>(),
                comment,
                "{source}"
            );
        }
    }

    #[test]
    fn viewport_keeps_quoted_multiline_comments_as_values() {
        let registry = ExtensionRegistry::new();
        let buffer = TextBuffer::from_text("KEY=\"primera\n# valor\nfinal\" # comentario\nNEXT=ok");
        for lines in [
            registry.highlight_source(&buffer, Some(Path::new(".env.local")), 1, 3),
            registry.highlight_viewport(&buffer, Some(Path::new(".env.local")), 1, 3),
        ] {
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[0].line_number, 2);
            assert_eq!(lines[0].spans[0].color, VALUE);
            assert!(lines[1]
                .spans
                .iter()
                .any(|s| s.color == COMMENT && s.text == "# comentario"));
            assert_eq!(lines[2].spans[0].color, KEY);
        }
    }
}
