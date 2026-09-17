use crate::buffer::TextBuffer;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;

/// A unique temporary file on the same volume; preserves target permissions.
pub fn save_atomic(target_path: &Path, content: &str) -> Result<()> {
    let parent = target_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = fs::metadata(target_path) {
        if metadata.permissions().readonly() {
            bail!("El archivo es de solo lectura");
        }
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.write_all(content.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(target_path)
        .context("No se pudo reemplazar el archivo; el original se conserva")?;
    Ok(())
}

pub fn save_buffer(buffer: &mut TextBuffer) -> Result<()> {
    let path = buffer.path.as_ref().context("No hay un archivo asociado")?;
    save_atomic(path, &buffer.to_string_with_endings())?;
    buffer.is_dirty = false;
    Ok(())
}

/// Refuse to overwrite changes made by an agent since the file was opened.
pub fn save_if_unchanged(buffer: &mut TextBuffer, original: &TextBuffer) -> Result<()> {
    let disk = TextBuffer::from_file(buffer.path.as_ref().context("No hay archivo abierto")?)?;
    if disk.rope != original.rope || disk.has_bom != original.has_bom {
        bail!("El archivo cambió en disco. Recarga y revisa los cambios antes de guardar.");
    }
    save_buffer(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_changes_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "KEY=old\r\n").unwrap();
        let original = TextBuffer::from_file(&path).unwrap();
        let mut edited = original.clone();
        edited.replace_line(0, "KEY=local").unwrap();
        fs::write(&path, "KEY=agent\r\n").unwrap();
        assert!(save_if_unchanged(&mut edited, &original).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "KEY=agent\r\n");
        fs::write(&path, "KEY=old\r\n").unwrap();
        save_if_unchanged(&mut edited, &original).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "KEY=local\r\n");
    }
}
