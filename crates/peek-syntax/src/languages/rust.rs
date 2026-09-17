use super::traits::{HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType};
use crate::themes::ColorRgb;

pub struct RustExtension;

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "yield",
];

const RUST_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "str", "String", "Option", "Result", "Vec", "Box", "Rc", "Arc",
    "RefCell", "Cell", "Mutex", "RwLock", "Path", "PathBuf", "Some", "None", "Ok", "Err",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Duration", "Instant",
];

impl LanguageExtension for RustExtension {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn name(&self) -> &'static str {
        "Rust"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        _ctx: &mut LineContext,
    ) -> HighlightedLine {
        let spans = tokenize_rust_line(line);
        HighlightedLine {
            line_number,
            line_type: LineType::Normal,
            spans,
            is_markdown: false,
        }
    }
}

pub fn tokenize_rust_line(line: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let col_kw = ColorRgb::new(192, 132, 252); // purple-400
    let col_type = ColorRgb::new(56, 189, 248); // sky-400
    let col_str = ColorRgb::new(163, 230, 53); // lime-400
    let col_comment = ColorRgb::new(113, 113, 122); // zinc-500
    let col_num = ColorRgb::new(251, 146, 60); // orange-400
    let col_macro = ColorRgb::new(234, 179, 8); // yellow-500
    let col_attr = ColorRgb::new(244, 63, 94); // rose-500
    let col_lifetime = ColorRgb::new(45, 212, 191); // teal-400
    let col_text = ColorRgb::new(228, 228, 231); // zinc-200

    while i < len {
        // 1. Comments: //
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            let comment_text: String = chars[i..].iter().collect();
            spans.push(HighlightSpan::italic(comment_text, col_comment));
            break;
        }

        // 2. Attributes: #[derive(...)] or #![...]
        if chars[i] == '#' && i + 1 < len && (chars[i + 1] == '[' || chars[i + 1] == '!') {
            let attr_text: String = chars[i..].iter().collect();
            spans.push(HighlightSpan::new(attr_text, col_attr));
            break;
        }

        // 3. String literals: "..." or r#"..."#
        if chars[i] == '"' {
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            spans.push(HighlightSpan::new(s, col_str));
            continue;
        }

        // 4. Lifetimes or char: 'a or 'static or '\n'
        if chars[i] == '\'' && i + 1 < len && chars[i + 1].is_alphabetic() {
            let start = i;
            i += 1;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let lifetime: String = chars[start..i].iter().collect();
            spans.push(HighlightSpan::new(lifetime, col_lifetime));
            continue;
        }

        // 5. Numbers: digits
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            let num: String = chars[start..i].iter().collect();
            spans.push(HighlightSpan::new(num, col_num));
            continue;
        }

        // 6. Identifiers / Keywords / Types / Macros
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();

            // Macro call: ident!
            if i < len && chars[i] == '!' {
                i += 1;
                let macro_name: String = chars[start..i].iter().collect();
                spans.push(HighlightSpan::bold(macro_name, col_macro));
                continue;
            }

            if RUST_KEYWORDS.contains(&ident.as_str()) {
                spans.push(HighlightSpan::bold(ident, col_kw));
            } else if RUST_TYPES.contains(&ident.as_str())
                || (ident
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                    && !ident.chars().all(|c| c.is_uppercase() || c == '_'))
            {
                spans.push(HighlightSpan::new(ident, col_type));
            } else {
                spans.push(HighlightSpan::new(ident, col_text));
            }
            continue;
        }

        // 7. Whitespace and symbols
        let start = i;
        while i < len
            && !chars[i].is_alphanumeric()
            && chars[i] != '_'
            && chars[i] != '"'
            && chars[i] != '\''
            && !(i + 1 < len && chars[i] == '/' && chars[i + 1] == '/')
        {
            i += 1;
        }
        let sym: String = chars[start..i].iter().collect();
        spans.push(HighlightSpan::new(sym, col_text));
    }

    spans
}
