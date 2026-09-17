pub mod atomic_save;
pub mod buffer;
pub mod projects;
pub mod settings;
pub mod workspace;

pub use atomic_save::{save_atomic, save_buffer};
pub use buffer::{LineEnding, TextBuffer};
pub use projects::{ProjectItem, ProjectManager};
pub use settings::UserSettings;
pub use workspace::Workspace;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    #[test]
    fn test_empty_buffer() {
        let buf = TextBuffer::new();
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.len_chars(), 0);
        assert!(!buf.is_dirty);
    }

    #[test]
    fn test_from_str_and_line_trimmed() {
        let text = "fn main() {\r\n    println!(\"Hello\");\r\n}";
        let buf = TextBuffer::from_text(text);
        assert_eq!(buf.line_ending, LineEnding::Crlf);
        assert_eq!(buf.line_count(), 3);
        assert_eq!(buf.line_trimmed(0).as_deref(), Some("fn main() {"));
        assert_eq!(
            buf.line_trimmed(1).as_deref(),
            Some("    println!(\"Hello\");")
        );
        assert_eq!(buf.line_trimmed(2).as_deref(), Some("}"));
        assert_eq!(buf.line_trimmed(3), None);
    }

    #[test]
    fn test_line_ending_detection() {
        assert_eq!(
            TextBuffer::detect_line_ending("line 1\nline 2\n"),
            LineEnding::Lf
        );
        assert_eq!(
            TextBuffer::detect_line_ending("line 1\r\nline 2\r\n"),
            LineEnding::Crlf
        );
    }

    #[test]
    fn test_insert_and_delete() {
        let mut buf = TextBuffer::from_text("Hello World");
        assert!(!buf.is_dirty);
        assert_eq!(buf.revision, 0);

        // Insert at index 5 (after Hello)
        buf.insert(5, " Beautiful");
        assert!(buf.is_dirty);
        assert_eq!(buf.revision, 1);
        assert_eq!(
            buf.line_trimmed(0).as_deref(),
            Some("Hello Beautiful World")
        );

        // Delete " Beautiful" (chars 5..15)
        buf.delete(5..15);
        assert_eq!(buf.revision, 2);
        assert_eq!(buf.line_trimmed(0).as_deref(), Some("Hello World"));
    }

    #[test]
    fn test_atomic_save() {
        let test_dir = std::env::temp_dir().join("codepeek_test_atomic");
        let _ = fs::create_dir_all(&test_dir);
        let test_file = test_dir.join("test_save.txt");

        let initial_text = "fn compute() -> u32 {\n    42\n}\n";
        fs::write(&test_file, initial_text).expect("Write initial test file");

        let mut buf = TextBuffer::from_file(&test_file).expect("Load buffer");
        assert!(!buf.is_dirty);

        // Modify buffer
        buf.insert(buf.len_chars() - 2, " // updated");
        assert!(buf.is_dirty);

        // Save atomically
        save_buffer(&mut buf).expect("Atomic save");
        assert!(!buf.is_dirty);

        // Verify disk contents
        let disk_content = fs::read_to_string(&test_file).expect("Read saved file");
        assert!(disk_content.contains("// updated"));

        // Cleanup
        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir(&test_dir);
    }

    #[test]
    fn test_50k_lines_access_speed() {
        let mut large_text = String::with_capacity(2_000_000);
        for i in 0..50_000 {
            large_text.push_str(&format!("Line {}: let value_{} = {};\n", i, i, i * 2));
        }

        let buf = TextBuffer::from_text(&large_text);
        assert_eq!(buf.line_count(), 50_001);

        let start = Instant::now();
        let middle_line = buf.line_trimmed(25_000);
        let elapsed = start.elapsed();

        assert!(middle_line.is_some());
        assert!(middle_line.unwrap().starts_with("Line 25000:"));
        // O(log N) line lookup should take less than 100 microseconds
        assert!(elapsed.as_micros() < 500, "Line lookup took {:?}", elapsed);
    }

    #[test]
    fn line_edit_preserves_unicode_bom_and_mixed_endings() {
        let mut buffer = TextBuffer::from_text("\u{feff}niño=uno\r\nsegundo=dos\ntercero=tres\r\n");
        buffer.replace_line(0, "niño=áé😀").unwrap();
        assert_eq!(
            buffer.to_string_with_endings(),
            "\u{feff}niño=áé😀\r\nsegundo=dos\ntercero=tres\r\n"
        );
        assert!(buffer.replace_line(1, "bad\nline").is_err());
        assert!(buffer.replace_line(20, "missing").is_err());
    }

    #[test]
    fn streaming_reader_rejects_binary_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("example.txt");
        let text = "\u{feff}hola\r\nniño😀\n";
        fs::write(&path, text).unwrap();
        assert_eq!(
            TextBuffer::from_file(&path)
                .unwrap()
                .to_string_with_endings(),
            text
        );
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(TextBuffer::from_file(&path).is_err());
        fs::write(&path, b"a\0b").unwrap();
        assert!(TextBuffer::from_file(&path).is_err());
    }
}
