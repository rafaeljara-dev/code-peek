use peek_core::TextBuffer;
use peek_syntax::{ExtensionRegistry, HighlightedLine};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(PartialEq, Eq)]
struct Key {
    path: Option<PathBuf>,
    revision: u64,
    start: usize,
    count: usize,
    preview: bool,
}

/// Retains only the active viewport. Explicit invalidation handles undo revision reuse.
#[derive(Default)]
pub struct HighlightCache {
    entry: Option<(Key, Arc<Vec<HighlightedLine>>)>,
}

impl HighlightCache {
    pub fn invalidate(&mut self) {
        self.entry = None;
    }

    pub fn get(
        &mut self,
        syntax: &ExtensionRegistry,
        buffer: &TextBuffer,
        path: Option<&Path>,
        start: usize,
        count: usize,
        preview: bool,
    ) -> Arc<Vec<HighlightedLine>> {
        let key = Key {
            path: path.map(Path::to_path_buf),
            revision: buffer.revision,
            start,
            count,
            preview,
        };
        if let Some((cached, lines)) = &self.entry {
            if *cached == key {
                return lines.clone();
            }
        }
        let lines = Arc::new(if preview {
            syntax.highlight_viewport(buffer, path, start, count)
        } else {
            syntax.highlight_source(buffer, path, start, count)
        });
        self.entry = Some((key, lines.clone()));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewport_cache_reuses_tokens_and_invalidates_content_path_range_and_mode() {
        let syntax = ExtensionRegistry::new();
        let mut buffer = TextBuffer::from_text("**niño**\nnext");
        let path = Some(Path::new("notes.md"));
        let mut cache = HighlightCache::default();
        let a = cache.get(&syntax, &buffer, path, 0, 2, false);
        assert!(Arc::ptr_eq(
            &a,
            &cache.get(&syntax, &buffer, path, 0, 2, false)
        ));
        let preview = cache.get(&syntax, &buffer, path, 0, 2, true);
        assert_ne!(a[0].spans, preview[0].spans);
        buffer.insert(0, "x");
        let edited = cache.get(&syntax, &buffer, path, 0, 2, false);
        assert_ne!(edited[0].spans, a[0].spans);
        let moved = cache.get(&syntax, &buffer, path, 1, 1, false);
        assert_eq!(moved[0].line_number, 2);
        let other = cache.get(&syntax, &buffer, Some(Path::new("notes.rs")), 1, 1, false);
        assert!(!Arc::ptr_eq(&moved, &other));
        cache.invalidate();
        assert!(!Arc::ptr_eq(
            &other,
            &cache.get(&syntax, &buffer, Some(Path::new("notes.rs")), 1, 1, false)
        ));
    }
}
