use super::dotenv::DotenvExtension;
use super::markdown::MarkdownExtension;
use super::nextjs::NextJsExtension;
use super::rust::RustExtension;
use super::traits::{HighlightedLine, LanguageExtension, LineContext};
use crate::highlighter::SyntaxHighlighter;
use peek_core::TextBuffer;
use std::path::Path;
use std::sync::Arc;

pub struct ExtensionRegistry {
    extensions: Vec<Arc<dyn LanguageExtension>>,
    fallback_syntect: SyntaxHighlighter,
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            extensions: Vec::new(),
            fallback_syntect: SyntaxHighlighter::new(),
        };

        // Register default first-class extensions
        registry.register(Arc::new(DotenvExtension));
        registry.register(Arc::new(MarkdownExtension));
        registry.register(Arc::new(RustExtension));
        registry.register(Arc::new(NextJsExtension));

        registry
    }

    /// Register a new language extension dynamically
    pub fn register(&mut self, ext: Arc<dyn LanguageExtension>) {
        self.extensions.push(ext);
    }

    /// Source-preserving highlighting for editing; Markdown formatting is preview-only.
    pub fn highlight_source(
        &self,
        buffer: &TextBuffer,
        path: Option<&Path>,
        start: usize,
        count: usize,
    ) -> Vec<HighlightedLine> {
        if let Some(extension) = path
            .and_then(|p| self.find_extension(p))
            .filter(|e| !e.is_markdown() && e.id() != "rust")
        {
            let mut context = LineContext::default();
            return (start.saturating_sub(64)..(start + count).min(buffer.line_count()))
                .filter_map(|line| {
                    let source = buffer.line_trimmed(line).unwrap_or_default();
                    let highlighted = extension.tokenize_line(&source, line + 1, &mut context);
                    (line >= start).then_some(highlighted)
                })
                .collect();
        }
        self.fallback_syntect
            .highlight_viewport(buffer, path, start, count)
    }

    pub fn language_name(&self, path: &Path) -> String {
        self.find_extension(path)
            .map(|e| e.name().to_owned())
            .unwrap_or_else(|| {
                path.extension()
                    .and_then(|s| s.to_str())
                    .and_then(|ext| {
                        self.fallback_syntect
                            .syntax_set
                            .find_syntax_by_extension(ext)
                    })
                    .map_or_else(|| "Texto plano".into(), |syntax| syntax.name.clone())
            })
    }

    /// Find matching language extension for a given path
    pub fn find_extension(&self, path: &Path) -> Option<Arc<dyn LanguageExtension>> {
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();

        // 1. Check special filenames (e.g. next.config.js, package.json)
        for e in &self.extensions {
            if e.matches_filename(file_name) {
                return Some(e.clone());
            }
        }

        // 2. Check file extensions (e.g. rs, md, tsx)
        if !ext.is_empty() {
            for e in &self.extensions {
                if e.file_extensions()
                    .iter()
                    .any(|&fe| fe.eq_ignore_ascii_case(&ext))
                {
                    return Some(e.clone());
                }
            }
        }

        None
    }

    /// Tokenize visible viewport lines with highest possible speed and accuracy
    pub fn highlight_viewport(
        &self,
        buffer: &TextBuffer,
        path: Option<&Path>,
        start_line: usize,
        count: usize,
    ) -> Vec<HighlightedLine> {
        let total_lines = buffer.line_count();
        if total_lines == 0 || start_line >= total_lines {
            return Vec::new();
        }

        let end_line = (start_line + count).min(total_lines);

        // Check if we have a dedicated extension
        if let Some(p) = path {
            if let Some(ext) = self.find_extension(p) {
                if ext.id() == "dotenv" {
                    return self.highlight_source(buffer, path, start_line, count);
                }
                let mut lines = Vec::with_capacity(end_line - start_line);
                let mut ctx = LineContext::default();

                for idx in start_line..end_line {
                    let content = buffer.line_trimmed(idx).unwrap_or_default();
                    let line = ext.tokenize_line(&content, idx + 1, &mut ctx);
                    lines.push(line);
                }

                return lines;
            }
        }

        // Fallback to Syntect
        self.fallback_syntect
            .highlight_viewport(buffer, path, start_line, count)
    }
}
