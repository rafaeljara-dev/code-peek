//! Isolated probes of production operations and reusable caches.
//! Run with `cargo run --release -p peek-ui --example performance_audit -- <case>`.

use anyhow::{bail, Result};
use peek_core::TextBuffer;
use peek_syntax::ExtensionRegistry;
use peek_ui::{editor, wrapping};
use std::hint::black_box;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn measure(name: &str, repetitions: usize, mut action: impl FnMut() -> usize) {
    let mut times = Vec::with_capacity(repetitions);
    let mut count = 0;
    for _ in 0..repetitions {
        let started = Instant::now();
        count = black_box(action());
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!("{{\"case\":\"{name}\",\"samples\":{repetitions},\"median_ms\":{:.6},\"min_ms\":{:.6},\"max_ms\":{:.6},\"result_count\":{count}}}", times[times.len() / 2], times[0], times[times.len() - 1]);
}

fn large_buffer() -> Result<TextBuffer> {
    let mut fixture = tempfile::NamedTempFile::new()?;
    {
        let mut output = std::io::BufWriter::new(fixture.as_file_mut());
        for _ in 0..500_000 {
            output.write_all(b"let answer = 42; // code peek\n")?;
        }
        output.flush()?;
    }
    let buffer = TextBuffer::from_file(fixture.path())?;
    println!(
        "{{\"fixture_lines\":{},\"fixture_bytes\":{}}}",
        buffer.line_count(),
        buffer.len_bytes()
    );
    Ok(buffer)
}

fn main() -> Result<()> {
    let case = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "syntax-init".into());
    match case.as_str() {
        "syntax-init" => measure("syntax_init", 7, || {
            black_box(ExtensionRegistry::new());
            1
        }),
        "viewport" => {
            let buffer = large_buffer()?;
            let syntax = ExtensionRegistry::new();
            let path = Some(Path::new("main.rs"));
            measure("source_viewport_40_deep", 50, || {
                syntax.highlight_source(&buffer, path, 499_900, 40).len()
            });
            measure("both_viewports_40_deep", 50, || {
                let a = syntax.highlight_source(&buffer, path, 499_900, 40);
                let b = syntax.highlight_viewport(&buffer, path, 499_900, 40);
                black_box(a.len() + b.len())
            });
            measure("rope_clone", 1000, || black_box(buffer.clone()).len_bytes());
            let mut cache = peek_ui::viewport::HighlightCache::default();
            cache.get(&syntax, &buffer, path, 499_900, 40, false);
            measure("cached_source_viewport_40_deep", 1000, || {
                cache.get(&syntax, &buffer, path, 499_900, 40, false).len()
            });
        }
        "editing" => {
            let original = large_buffer()?;
            for (name, offset) in [
                ("dirty_compare_first_same_length", 0),
                ("dirty_compare_last_same_length", original.len_chars() - 2),
            ] {
                let mut edited = original.clone();
                edited.delete(offset..offset + 1);
                edited.insert(offset, "x");
                measure(name, 100, || {
                    usize::from(black_box(&edited.rope) != black_box(&original.rope))
                });
            }
            measure("insert_last_with_dirty_check", 100, || {
                let mut buffer = original.clone();
                let mut editor = editor::EditorState::default();
                editor.cursor = buffer.len_chars();
                editor.anchor = editor.cursor;
                let snapshot = editor::EditSnapshot::new(buffer.clone(), &editor);
                editor.command(&mut buffer, "insert", "x", false, false);
                black_box(snapshot);
                usize::from(buffer.rope != original.rope)
            });
            measure("replace_last_with_dirty_check", 100, || {
                let mut buffer = original.clone();
                let mut editor = editor::EditorState::default();
                editor.cursor = buffer.len_chars() - 1;
                editor.anchor = editor.cursor - 1;
                let snapshot = editor::EditSnapshot::new(buffer.clone(), &editor);
                editor.command(&mut buffer, "paste", "x", false, false);
                black_box(snapshot);
                usize::from(buffer.rope != original.rope)
            });
            measure("find_all_500k_lines", 5, || {
                editor::matches(&original, "42", false, false).len()
            });
        }
        "fuzzy" => {
            let paths: Vec<PathBuf> = (0..100_000)
                .map(|index| {
                    PathBuf::from(format!("src/group_{}/component_{index}.tsx", index / 100))
                })
                .collect();
            let mut search = peek_tree::FuzzySearcher::new();
            measure("fuzzy_100k_many_matches", 7, || {
                search.search("src", &paths, 60).len()
            });
            measure("fuzzy_100k_specific", 7, || {
                search.search("component_999", &paths, 60).len()
            });
        }
        "image-copy" => {
            let raw =
                peek_ui::image_loader::RawRgbaImage::new(vec![255; 4096 * 4096 * 4], 4096, 4096);
            measure("image_convert_4096_rgba", 7, || {
                let image = raw.to_slint_image();
                black_box(image.size().width as usize)
            });
        }
        "minified-visible" | "minified-full" => {
            for mib in [4, 16] {
                let source = "x".repeat(mib * 1024 * 1024);
                let spans = [peek_syntax::HighlightSpan::new(
                    &source,
                    peek_syntax::ColorRgb::new(1, 2, 3),
                )];
                let count = if case == "minified-full" {
                    usize::MAX
                } else {
                    40
                };
                measure(
                    &format!(
                        "wrap_{mib}mib_{}",
                        if count == 40 { "visible40" } else { "all_rows" }
                    ),
                    7,
                    || {
                        wrapping::wrap_visible(&source, &spans, 100, false, 0, count)
                            .1
                            .len()
                    },
                );
            }
        }
        "thumbnail" => {
            let fixture = tempfile::Builder::new().suffix(".png").tempfile()?;
            {
                let data = vec![240; 4096 * 4096 * 4];
                image::save_buffer(fixture.path(), &data, 4096, 4096, image::ColorType::Rgba8)?;
            }
            println!(
                "{{\"compressed_image_bytes\":{},\"decoded_rgba_bytes\":{}}}",
                fixture.as_file().metadata()?.len(),
                4096 * 4096 * 4
            );
            measure("thumbnail_4096_to_36", 5, || {
                peek_ui::image_loader::load_raster_thumb_raw(fixture.path(), 36)
                    .unwrap()
                    .width() as usize
            });
        }
        "load-save" => {
            let generated = large_buffer()?;
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("large.rs");
            {
                let mut output = std::io::BufWriter::new(std::fs::File::create(&path)?);
                for chunk in generated.rope.chunks() {
                    output.write_all(chunk.as_bytes())?;
                }
                output.flush()?;
            }
            drop(generated);
            measure("load_utf8_500k_lines", 5, || {
                TextBuffer::from_file(&path).unwrap().len_bytes()
            });
            let original = TextBuffer::from_file(&path)?;
            let mut edited = original.clone();
            edited.insert(0, "x");
            measure("save_conflict_check_and_atomic", 1, || {
                peek_core::atomic_save::save_if_unchanged(&mut edited, &original).unwrap();
                edited.len_bytes()
            });
        }
        _ => bail!("Unknown case: {case}"),
    }
    Ok(())
}
