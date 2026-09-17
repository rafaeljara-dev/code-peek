use crate::atomic_save::save_buffer;
use crate::buffer::TextBuffer;
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Workspace {
    pub root_dir: Option<PathBuf>,
    pub current_file: Option<PathBuf>,
    pub buffer: Option<TextBuffer>,
}

impl Workspace {
    pub fn new(root_dir: Option<PathBuf>) -> Self {
        Self {
            root_dir,
            current_file: None,
            buffer: None,
        }
    }

    /// Opens a file from disk into the active workspace buffer.
    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        let buffer = TextBuffer::from_file(path)?;
        self.current_file = Some(path.to_path_buf());
        self.buffer = Some(buffer);
        Ok(())
    }

    /// Closes the currently active file buffer.
    pub fn close_file(&mut self) {
        self.current_file = None;
        self.buffer = None;
    }

    /// Saves the current buffer to disk atomically.
    pub fn save_current(&mut self) -> Result<()> {
        if let Some(ref mut buf) = self.buffer {
            save_buffer(buf)?;
        }
        Ok(())
    }

    /// Returns true if the active buffer has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.buffer.as_ref().map(|b| b.is_dirty).unwrap_or(false)
    }
}
