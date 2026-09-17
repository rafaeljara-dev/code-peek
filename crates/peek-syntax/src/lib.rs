pub mod highlighter;
pub mod languages;
pub mod themes;

pub use highlighter::SyntaxHighlighter;
pub use languages::{
    ExtensionRegistry, HighlightSpan, HighlightedLine, LanguageExtension, LineContext, LineType,
    MarkdownExtension, NextJsExtension, RustExtension,
};
pub use themes::{ColorRgb, EditorTheme};

#[cfg(test)]
mod tests {
    use super::*;
    use peek_core::TextBuffer;
    use std::path::Path;

    #[test]
    fn test_theme_definition() {
        let theme = EditorTheme::fluent_dark();
        assert_eq!(theme.name, "Fluent Dark");
        assert_eq!(theme.background, ColorRgb::new(18, 18, 20));
        assert_eq!(theme.background.to_hex(), "#121214");
    }

    #[test]
    fn test_rust_highlighting() {
        let code = "fn main() {\n    let answer = 42;\n}";
        let buffer = TextBuffer::from_text(code);
        let registry = ExtensionRegistry::new();

        let highlighted = registry.highlight_viewport(&buffer, Some(Path::new("main.rs")), 0, 3);

        assert_eq!(highlighted.len(), 3);
        assert_eq!(highlighted[0].line_number, 1);
        assert!(!highlighted[0].spans.is_empty());
        assert_eq!(highlighted[0].spans[0].text, "fn");
        assert!(highlighted[0].spans[0].bold);
    }

    #[test]
    fn test_nextjs_tsx_highlighting() {
        let code = "export default function Page() {\n    const [count, setCount] = useState(0);\n    return <div className=\"p-4\">Count</div>;\n}";
        let buffer = TextBuffer::from_text(code);
        let registry = ExtensionRegistry::new();

        let highlighted = registry.highlight_viewport(&buffer, Some(Path::new("page.tsx")), 0, 4);

        assert_eq!(highlighted.len(), 4);
        // Verify useState hook is highlighted in line 2
        let line2_text: String = highlighted[1]
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(line2_text.contains("useState"));
        // Verify JSX tag <div is identified in line 3
        let has_jsx_div = highlighted[2].spans.iter().any(|s| s.text == "<div");
        assert!(has_jsx_div);
    }

    #[test]
    fn test_markdown_formatting() {
        let md = "# Title\n> Quote\n- List item\n```rust\nlet x = 1;\n```";
        let buffer = TextBuffer::from_text(md);
        let registry = ExtensionRegistry::new();

        let highlighted = registry.highlight_viewport(&buffer, Some(Path::new("README.md")), 0, 5);

        assert_eq!(highlighted.len(), 5);
        assert_eq!(highlighted[0].line_type, LineType::Heading1);
        assert_eq!(highlighted[1].line_type, LineType::Blockquote);
        assert_eq!(highlighted[2].line_type, LineType::ListItem);
        assert_eq!(highlighted[3].line_type, LineType::CodeFence);
    }

    #[test]
    fn test_viewport_line_selection() {
        let code = "line 1\nline 2\nline 3\nline 4\nline 5\n";
        let buffer = TextBuffer::from_text(code);
        let registry = ExtensionRegistry::new();

        let highlighted = registry.highlight_viewport(
            &buffer,
            Some(Path::new("test.txt")),
            2, // start at line 3 (index 2)
            2, // count 2 lines
        );

        assert_eq!(highlighted.len(), 2);
        assert_eq!(highlighted[0].line_number, 3);
        assert_eq!(highlighted[1].line_number, 4);
    }
}
