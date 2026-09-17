use super::traits::{HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType};
use crate::themes::ColorRgb;

pub struct NextJsExtension;

const JS_KEYWORDS: &[&str] = &[
    "import",
    "export",
    "from",
    "default",
    "as",
    "const",
    "let",
    "var",
    "function",
    "return",
    "if",
    "else",
    "async",
    "await",
    "type",
    "interface",
    "class",
    "extends",
    "implements",
    "new",
    "typeof",
    "instanceof",
    "try",
    "catch",
    "finally",
    "throw",
    "switch",
    "case",
    "break",
    "continue",
    "yield",
    "null",
    "undefined",
    "true",
    "false",
    "keyof",
    "in",
    "of",
    "delete",
    "void",
    "static",
    "readonly",
];

const REACT_NEXT_SPECIAL: &[&str] = &[
    "useState",
    "useEffect",
    "useMemo",
    "useCallback",
    "useRef",
    "useContext",
    "useReducer",
    "useId",
    "useRouter",
    "useSearchParams",
    "usePathname",
    "useParams",
    "cookies",
    "headers",
    "redirect",
    "notFound",
    "metadata",
    "generateMetadata",
    "generateStaticParams",
    "revalidateTag",
    "revalidatePath",
    "Link",
    "Image",
    "Script",
    "Head",
    "FC",
    "ReactNode",
    "NextPage",
    "NextRequest",
    "NextResponse",
    "GetServerSideProps",
    "GetStaticProps",
];

const TS_TYPES: &[&str] = &[
    "string", "number", "boolean", "any", "void", "never", "unknown", "object", "symbol", "Record",
    "Array", "Promise", "Partial", "Required", "Pick", "Omit", "Exclude", "Extract",
];

impl LanguageExtension for NextJsExtension {
    fn id(&self) -> &'static str {
        "nextjs"
    }

    fn name(&self) -> &'static str {
        "Next.js / React (TypeScript & JSX)"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["tsx", "jsx", "ts", "js", "mjs", "cjs", "json", "css"]
    }

    fn special_filenames(&self) -> &'static [&'static str] {
        &[
            "next.config.js",
            "next.config.mjs",
            "next.config.ts",
            "package.json",
            "tsconfig.json",
            "tailwind.config.js",
            "tailwind.config.ts",
        ]
    }

    fn tokenize_line(
        &self,
        line: &str,
        line_number: usize,
        _ctx: &mut LineContext,
    ) -> HighlightedLine {
        let spans = tokenize_nextjs_line(line);
        HighlightedLine {
            line_number,
            line_type: LineType::Normal,
            spans,
            is_markdown: false,
        }
    }
}

pub fn tokenize_nextjs_line(line: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let col_kw = ColorRgb::new(192, 132, 252); // purple-400 (import, export, const...)
    let col_hook = ColorRgb::new(244, 63, 94); // rose-500 (useState, useRouter...)
    let col_jsx_tag = ColorRgb::new(56, 189, 248); // sky-400 (<div, <Component...)
    let col_attr = ColorRgb::new(251, 146, 60); // orange-400 (className, onClick...)
    let col_type = ColorRgb::new(96, 165, 250); // blue-400 (TypeScript types)
    let col_str = ColorRgb::new(163, 230, 53); // lime-400 (strings)
    let col_comment = ColorRgb::new(113, 113, 122); // zinc-500 (comments)
    let col_num = ColorRgb::new(251, 146, 60); // orange-400 (numbers)
    let col_text = ColorRgb::new(228, 228, 231); // zinc-200

    while i < len {
        // 1. Comments: // or /*
        if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
            let comment_text: String = chars[i..].iter().collect();
            spans.push(HighlightSpan::italic(comment_text, col_comment));
            break;
        }

        // 2. JSX Tag start: <tag or </tag
        if chars[i] == '<' {
            let start = i;
            i += 1;
            if i < len && chars[i] == '/' {
                i += 1;
            }
            if i < len && (chars[i].is_alphabetic() || chars[i] == '_') {
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                let tag_name: String = chars[start..i].iter().collect();
                spans.push(HighlightSpan::bold(tag_name, col_jsx_tag));
                continue;
            } else {
                spans.push(HighlightSpan::new("<", col_text));
                continue;
            }
        }

        // 3. String literals: "...", '...', `...`
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let quote = chars[i];
            let start = i;
            i += 1;
            while i < len {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            spans.push(HighlightSpan::new(s, col_str));
            continue;
        }

        // 4. Numbers
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                i += 1;
            }
            let num: String = chars[start..i].iter().collect();
            spans.push(HighlightSpan::new(num, col_num));
            continue;
        }

        // 5. Identifiers: keywords, hooks, types, props
        if chars[i].is_alphabetic() || chars[i] == '_' || chars[i] == '$' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();

            // Next token check for JSX prop: `className=`
            if i < len && chars[i] == '=' {
                spans.push(HighlightSpan::new(ident, col_attr));
                continue;
            }

            if REACT_NEXT_SPECIAL.contains(&ident.as_str()) {
                spans.push(HighlightSpan::bold(ident, col_hook));
            } else if JS_KEYWORDS.contains(&ident.as_str()) {
                spans.push(HighlightSpan::bold(ident, col_kw));
            } else if TS_TYPES.contains(&ident.as_str())
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

        // 6. Whitespace and symbols
        let start = i;
        while i < len
            && !chars[i].is_alphanumeric()
            && chars[i] != '_'
            && chars[i] != '$'
            && chars[i] != '"'
            && chars[i] != '\''
            && chars[i] != '`'
            && chars[i] != '<'
            && !(i + 1 < len && chars[i] == '/' && chars[i + 1] == '/')
        {
            i += 1;
        }
        let sym: String = chars[start..i].iter().collect();
        spans.push(HighlightSpan::new(sym, col_text));
    }

    spans
}
