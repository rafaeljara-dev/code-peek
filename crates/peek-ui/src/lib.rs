mod controller;
mod doc_view;
// Public only for the performance probes in `examples/`; not a stable API.
#[doc(hidden)]
pub mod editor;
pub mod image_loader;
mod pdf_view;
mod sheet_view;
mod theme;
mod thumbnails;
pub mod viewport;
#[doc(hidden)]
pub mod wrapping;

use controller::{Command, Controller};
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::PathBuf;
use std::sync::mpsc;

slint::include_modules!();

pub fn run_app(initial_path: Option<PathBuf>) -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    window.set_code_font_family(
        if cfg!(target_os = "windows") {
            "Consolas"
        } else {
            "monospace"
        }
        .into(),
    );
    let (tx, rx) = mpsc::channel();
    let weak = window.as_weak();
    let worker_tx = tx.clone();
    std::thread::spawn(move || Controller::run(initial_path, weak, rx, worker_tx));
    {
        // Files opened from the shell while this window is running arrive here as tabs.
        let tx = tx.clone();
        peek_core::single_instance::listen(move |path| {
            let _ = tx.send(Command::OpenFile(path));
        });
    }

    macro_rules! connect {
        ($method:ident, || $command:expr) => {{
            let tx = tx.clone();
            window.$method(move || { let _ = tx.send($command); });
        }};
        ($method:ident, |$($arg:ident),*| $command:expr) => {{
            let tx = tx.clone();
            window.$method(move |$($arg),*| { let _ = tx.send($command); });
        }};
    }
    connect!(on_open_project, |p| Command::OpenProject(PathBuf::from(
        p.as_str()
    )));
    connect!(on_open_file, |p| Command::OpenFile(PathBuf::from(
        p.as_str()
    )));
    connect!(on_open_at, |p, line| Command::OpenAt(
        PathBuf::from(p.as_str()),
        line.max(1) as usize - 1
    ));
    connect!(on_close_file, |p| Command::CloseFile(PathBuf::from(
        p.as_str()
    )));
    connect!(on_move_tab, |from, to| Command::MoveTab(
        from.max(0) as usize,
        to.max(0) as usize
    ));
    connect!(on_close_project, |p| Command::CloseProject(PathBuf::from(
        p.as_str()
    )));
    connect!(on_toggle_folder, |p, library| Command::Toggle(
        PathBuf::from(p.as_str()),
        library
    ));
    connect!(on_set_library, |p| Command::Library(PathBuf::from(
        p.as_str()
    )));
    connect!(on_set_default, || Command::SetDefault);
    connect!(on_go_default, || Command::GoDefault);
    connect!(on_library_parent, || Command::LibraryParent);
    connect!(on_search, |q, mode| Command::Search(q.to_string(), mode));
    connect!(on_save, || Command::Save);
    connect!(on_document_edited, |path, text| Command::DocumentEdited(
        PathBuf::from(path.as_str()),
        text.to_string()
    ));
    connect!(on_revert, || Command::Revert);
    connect!(on_reload, || Command::Reload);
    connect!(on_edit_line, |n| Command::Edit(n.max(1) as usize - 1));
    connect!(on_select_line, |n| Command::Select(n.max(1) as usize - 1));
    connect!(on_apply_edit, |text| Command::Apply(text.to_string()));
    connect!(on_resolve_pending, |choice| Command::Resolve(choice));
    connect!(on_scroll_lines, |n| Command::Scroll(n));
    connect!(on_editor_scroll, |n| Command::EditorScroll(n));
    connect!(on_editor_scroll_to, |ratio| Command::EditorScrollTo(ratio));
    connect!(on_editor_pan, |n| Command::EditorPan(n));
    connect!(on_scroll_to, |ratio| Command::ScrollTo(ratio));
    connect!(
        on_viewport_changed,
        |n| Command::Viewport(n.max(1) as usize)
    );
    connect!(on_zoom, |delta| Command::Zoom(delta));
    connect!(on_zoom_ui, |delta| Command::ZoomUi(delta));
    connect!(on_toggle_theme, || Command::ToggleTheme);
    connect!(on_set_panels, |projects, explorer| Command::Panels(
        projects, explorer
    ));
    connect!(on_wrap_columns_changed, |columns| Command::WrapColumns(
        columns.max(4) as usize
    ));
    connect!(on_undo, || Command::Undo);
    connect!(on_redo, || Command::Redo);
    connect!(on_editor_command, |path, action, text, shift, control| {
        Command::EditorAction(
            PathBuf::from(path.as_str()),
            action.to_string(),
            text.to_string(),
            shift,
            control,
        )
    });
    connect!(on_editor_point, |path, line, column, extend| {
        Command::EditorPoint(
            PathBuf::from(path.as_str()),
            line.max(1) as usize - 1,
            column.max(0) as usize,
            extend,
        )
    });
    connect!(on_editor_copy, |cut| Command::EditorCopy(cut));
    connect!(on_go_line, |line| Command::GoLine(line.to_string()));
    connect!(on_find_advanced, |query, forward, case, whole| {
        Command::FindAdvanced(query.to_string(), forward, case, whole)
    });
    connect!(on_replace_text, |query, replacement, all, case, whole| {
        Command::Replace(query.to_string(), replacement.to_string(), all, case, whole)
    });
    connect!(on_find_text, |text, forward| Command::Find(
        text.to_string(),
        forward
    ));
    connect!(on_cancel_edit, || Command::CancelEdit);
    connect!(on_preview_changed, |preview| Command::Preview(preview));
    connect!(on_tree_viewport_changed, |first, count| {
        Command::TreeViewport(first.max(0) as usize, count.max(1) as usize)
    });
    connect!(on_select_doc_block, |idx, extend| Command::SelectDocBlock(
        idx.max(0) as usize,
        extend
    ));
    connect!(on_select_doc_all, || Command::SelectDocAll);
    connect!(on_copy_doc_selection, || Command::CopyDocSelection(false));
    connect!(on_copy_doc_all, || Command::CopyDocSelection(true));
    connect!(on_jump_doc_block, |idx| Command::JumpDocBlock(
        idx.max(0) as usize
    ));
    connect!(on_step_doc_window, |forward| Command::StepDocWindow(
        forward
    ));
    connect!(on_open_doc_link, |url| Command::OpenDocLink(
        url.to_string()
    ));
    connect!(on_toggle_markdown_default_preview, || {
        Command::ToggleMarkdownDefaultPreview
    });
    connect!(on_toggle_single_file_mode, || Command::ToggleSingleFileMode);
    connect!(on_set_windows_integration, |context, md, code, docs| {
        Command::SetWindowsIntegration(context, md, code, docs)
    });
    connect!(on_apply_windows_integration, || {
        Command::ApplyWindowsIntegration
    });
    connect!(on_select_sheet, |index| Command::SelectSheet(
        index.max(0) as usize
    ));
    connect!(on_step_sheet_rows, |forward| Command::StepSheetRows(
        forward
    ));
    connect!(on_step_sheet_cols, |forward| Command::StepSheetCols(
        forward
    ));
    connect!(on_select_sheet_cell, |row, col| Command::SelectSheetCell(
        row.max(0) as usize,
        col.max(0) as usize
    ));
    connect!(on_copy_sheet_cell, || Command::CopySheetCell(false));
    connect!(on_copy_sheet_row, || Command::CopySheetCell(true));
    connect!(on_pdf_step_page, |delta| Command::PdfStepPage(delta));
    connect!(on_pdf_go_page, |page| Command::PdfGoPage(page.to_string()));
    connect!(on_pdf_zoom, |delta| Command::PdfZoom(delta));
    connect!(on_copy_pdf_text, || Command::CopyPdfText);
    connect!(on_pdf_wheel_page, |delta| Command::PdfWheelPage(delta));
    connect!(on_pdf_thumbs_viewport, |first, count| {
        Command::PdfThumbsViewport(first.max(0) as usize, count.max(1) as usize)
    });
    connect!(on_zoom_doc, |delta| Command::DocZoom(delta));
    connect!(on_toggle_code_block, |index| Command::ToggleCodeBlock(
        index.max(0) as usize
    ));
    {
        // Pages are rasterized for the real screen: report its pixel ratio with the width.
        let tx = tx.clone();
        let weak = window.as_weak();
        window.on_reading_width_changed(move |width| {
            let pixel_ratio = weak.upgrade().map_or(1.0, |w| w.window().scale_factor());
            let _ = tx.send(Command::ReadingWidth(width, pixel_ratio));
        });
    }
    {
        let weak = window.as_weak();
        window.on_browse_folder(move |start| {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected = rfd::FileDialog::new()
                    .set_title("Elegir carpeta para CodePeek")
                    .set_directory(start.as_str())
                    .pick_folder();
                let _ = weak.upgrade_in_event_loop(move |w| {
                    w.set_browsing(false);
                    if let Some(path) = selected {
                        w.set_path_input(path.to_string_lossy().as_ref().into());
                    }
                });
            });
        });
    }
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(w) = weak.upgrade() {
            if w.get_show_edit() || w.get_browsing() {
                w.set_error_message("Termina o cancela la edición antes de cerrar.".into());
                return CloseRequestResponse::KeepWindowShown;
            }
        }
        let _ = tx.send(Command::Close);
        CloseRequestResponse::KeepWindowShown
    });
    window.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::{ModelRc, VecModel};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn editor_routes_navigation_clipboard_and_focus_after_dialogs() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1320.0, 820.0));
        window.set_selected_path("example.rs".into());
        window.show().unwrap();
        let commands = Rc::new(RefCell::new(Vec::new()));
        let captured = commands.clone();
        window.on_editor_command(move |_, action, text, shift, control| {
            captured
                .borrow_mut()
                .push((action.to_string(), text.to_string(), shift, control))
        });
        let key = |key: slint::platform::Key, pressed: bool| {
            window.window().dispatch_event(if pressed {
                slint::platform::WindowEvent::KeyPressed { text: key.into() }
            } else {
                slint::platform::WindowEvent::KeyReleased { text: key.into() }
            });
        };
        key(slint::platform::Key::RightArrow, true);
        key(slint::platform::Key::RightArrow, false);
        assert_eq!(commands.borrow().last().unwrap().0, "right");
        key(slint::platform::Key::Shift, true);
        key(slint::platform::Key::LeftArrow, true);
        key(slint::platform::Key::LeftArrow, false);
        key(slint::platform::Key::Shift, false);
        assert!(commands.borrow().last().unwrap().2);
        window.invoke_write_clipboard("á\n😀".into());
        window.invoke_paste_content();
        assert_eq!(commands.borrow().last().unwrap().0, "paste");
        assert_eq!(commands.borrow().last().unwrap().1, "á\n😀");
        window.set_show_find(true);
        window.invoke_focus_content();
        key(slint::platform::Key::RightArrow, true);
        key(slint::platform::Key::RightArrow, false);
        assert_eq!(commands.borrow().last().unwrap().0, "right");
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                text: slint::platform::Key::Escape.into(),
            });
        assert!(!window.get_show_find());
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "ñ".into() });
        assert_eq!(commands.borrow().last().unwrap().1, "ñ");
    }

    #[test]
    fn native_ui_matrix_renders_editor_and_keeps_actions_inside_window() {
        slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some("software".into()),
            },
        )))
        .unwrap();
        let window = MainWindow::new().unwrap();
        window.set_selected_path("C:/sample/main.rs".into());
        window.set_current_file_path("src/main.rs".into());
        window.set_project_name("Proyecto de ejemplo".into());
        window.set_project_path("C:/sample".into());
        window.set_language_name("Rust".into());
        window.set_busy(false);
        window.set_is_dirty(true);
        window.set_can_undo(true);
        window.set_can_redo(true);
        window.set_total_lines(500_000);
        window.set_editor_visible_ratio(0.0001);
        window.set_status_info("UTF-8 · LF · Ctrl+S guardar".into());
        window.set_recent_projects(ModelRc::new(VecModel::from(vec![ProjectItem {
            path: "C:/sample".into(),
            display_path: "Proyecto de ejemplo".into(),
            ..Default::default()
        }])));
        window.set_recent_files(ModelRc::new(VecModel::from(
            [
                "main.rs",
                "README.md",
                "page.tsx",
                "settings.json",
                "styles.css",
            ]
            .map(|name| SearchItem {
                path: format!("C:/sample/{name}").into(),
                display_path: name.into(),
                ..Default::default()
            })
            .to_vec(),
        )));
        let code =
            "fn main() {\n    let answer = 42;\n    println!(\"¡Hola, mundo! {answer}\");\n}\n"
                .repeat(12);
        let buffer = peek_core::TextBuffer::from_text(&code);
        let syntax = peek_syntax::ExtensionRegistry::new();
        let highlighted =
            syntax.highlight_source(&buffer, Some(std::path::Path::new("main.rs")), 0, 45);
        for (width, font) in [(1320.0, 15.0), (960.0, 15.0), (1320.0, 22.0), (960.0, 22.0)] {
            window
                .window()
                .set_size(slint::LogicalSize::new(width, 820.0));
            window.set_ui_font_size(font);
            for light in [false, true] {
                window.set_light_mode(light);
                window.set_editor_rows(ModelRc::new(VecModel::from(
                    highlighted
                        .iter()
                        .map(|line| EditorRowItem {
                            line_number: line.line_number as i32,
                            selection_start: if line.line_number == 2 { 4 } else { 0 },
                            selection_end: if line.line_number == 2 { 13 } else { 0 },
                            spans: ModelRc::new(VecModel::from(
                                editor::display_spans(
                                    &buffer.line_trimmed(line.line_number - 1).unwrap(),
                                    &line.spans,
                                    0,
                                    100,
                                )
                                .into_iter()
                                .map(|s| EditorSpanItem {
                                    text: s.text.into(),
                                    column: s.column as i32,
                                    color: theme::foreground(
                                        s.color.r, s.color.g, s.color.b, light,
                                    ),
                                })
                                .collect::<Vec<_>>(),
                            )),
                        })
                        .collect::<Vec<_>>(),
                )));
                for replace in [false, true] {
                    window.set_show_find(replace);
                    window.set_show_replace(replace);
                    window.show().unwrap();
                    let snapshot = window.window().take_snapshot().unwrap();
                    if let Ok(directory) = std::env::var("CODEPEEK_AUDIT_DIR") {
                        std::fs::create_dir_all(&directory).unwrap();
                        let name = format!(
                            "{width}-{font}-{}-{}.png",
                            if light { "claro" } else { "oscuro" },
                            if replace { "reemplazo" } else { "editor" }
                        );
                        image::save_buffer(
                            std::path::Path::new(&directory).join(name),
                            snapshot.as_bytes(),
                            snapshot.width(),
                            snapshot.height(),
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                    }
                    for label in [
                        "Deshacer (Ctrl+Z)",
                        "Guardar cambios (Ctrl+S)",
                        "Buscar en el archivo (Ctrl+F)",
                    ] {
                        let element = i_slint_backend_testing::ElementQuery::from_root(&window)
                            .match_descendants()
                            .match_predicate(move |e| {
                                e.accessible_label().is_some_and(|s| s == label)
                            })
                            .find_first()
                            .unwrap();
                        let position = element.absolute_position();
                        let size = element.size();
                        assert!(
                            position.x >= 0.0 && position.x + size.width <= width + 1.0,
                            "{label} overflows at {width}px / {font}px: {position:?} {size:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn palette_focus_typing_keyboard_selection_and_empty_results() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1100.0, 760.0));
        window.show().unwrap();
        let queries = Rc::new(RefCell::new(Vec::new()));
        let captured = queries.clone();
        window.on_search(move |text, mode| captured.borrow_mut().push((text.to_string(), mode)));
        window.invoke_open_palette(1);
        assert_eq!(window.get_palette_mode(), 1);
        // Force layout/instantiation of the conditional input before key dispatch.
        let input = i_slint_backend_testing::ElementQuery::from_root(&window)
            .match_descendants()
            .match_type_name("LineEdit")
            .find_first()
            .unwrap();
        assert!(input.size().width > 100.0);
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "a".into() });
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "a".into() });
        assert_eq!(
            window.get_query().as_str(),
            "a",
            "The palette must receive typing immediately"
        );
        assert_eq!(queries.borrow().last(), Some(&("a".into(), 1)));
        window.set_search_results(ModelRc::new(VecModel::from(vec![
            SearchItem {
                path: "one".into(),
                display_path: "one".into(),
                line_number: 0,
            },
            SearchItem {
                path: "two".into(),
                display_path: "two".into(),
                line_number: 0,
            },
        ])));
        let opened = Rc::new(RefCell::new(String::new()));
        let captured = opened.clone();
        window.on_open_file(move |path| *captured.borrow_mut() = path.to_string());
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::DownArrow.into(),
            });
        assert_eq!(window.get_result_index(), 1);
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Return.into(),
            });
        assert_eq!(*opened.borrow(), "two");
        assert_eq!(window.get_palette_mode(), 0);
        window.invoke_open_palette(2);
        window.set_search_results(ModelRc::new(VecModel::from(Vec::<SearchItem>::new())));
        window.invoke_choose_result();
        assert_eq!(window.get_palette_mode(), 2);
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                text: slint::platform::Key::Escape.into(),
            });
        assert_eq!(window.get_palette_mode(), 0);
    }

    #[test]
    fn editor_accepts_typing_without_entering_a_line_dialog() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1100.0, 760.0));
        window.set_selected_path("example.txt".into());
        window.set_document_text("".into());
        window.show().unwrap();
        let edits = Rc::new(RefCell::new(Vec::new()));
        let captured = edits.clone();
        window.on_editor_command(move |path, action, text, _, _| {
            captured
                .borrow_mut()
                .push((path.to_string(), action.to_string(), text.to_string()))
        });
        let editor = i_slint_backend_testing::ElementQuery::from_root(&window)
            .match_descendants()
            .match_predicate(|element| {
                element
                    .accessible_label()
                    .is_some_and(|label| label.starts_with("Editor de código:"))
            })
            .find_first()
            .unwrap();
        assert!(editor.size().width > 300.0);
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: "ñ".into() });
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: "ñ".into() });
        assert!(window.get_document_text().is_empty());
        assert_eq!(
            edits.borrow().last(),
            Some(&("example.txt".into(), "insert".into(), "ñ".into()))
        );
        assert!(!window.get_show_edit());
        for label in ["Buscar archivos o texto", "Recargar (F5)"] {
            let button = i_slint_backend_testing::ElementQuery::from_root(&window)
                .match_descendants()
                .match_predicate(move |element| {
                    element.accessible_label().is_some_and(|text| text == label)
                })
                .find_first()
                .unwrap();
            assert!(
                button.size().width <= 60.0,
                "{label} must stay compact: {:?}",
                button.size()
            );
        }
    }

    #[test]
    fn markdown_blocks_never_overlap_and_size_to_content() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1180.0, 820.0));
        window.set_selected_path("C:/sample/NOTAS.md".into());
        window.set_current_file_path("NOTAS.md".into());
        window.set_markdown_file(true);
        window.set_preview(true);

        let syntax = peek_syntax::ExtensionRegistry::new();
        let md = concat!(
            "# Encabezado principal largo de prueba\n\n",
            "Primer párrafo con `código en línea`, **negrita** y texto largo suficiente para envolver en múltiples líneas.\n\n",
            "- Elemento uno con `token.rs`\n",
            "- Elemento dos con detalle extra\n\n",
            "## Segunda sección\n\n",
            "> [!NOTE]\n> Nota importante separada claramente.\n\n",
            "```rust\nfn hola() {\n    println!(\"mundo\");\n}\n```\n"
        );
        let mut doc_state = crate::doc_view::DocViewState::default();
        doc_state.ensure_markdown_parsed(md, 1, None, None, &syntax);
        let prepared = doc_state.prepare_models(crate::doc_view::DocLayout {
            width: 900.0,
            font: 15.0,
            zoom: 1.0,
        });
        let count = prepared.blocks.len();
        let (blocks, headings) = prepared.into_slint(false);
        window.set_doc_blocks(ModelRc::new(VecModel::from(blocks)));
        window.set_doc_headings(ModelRc::new(VecModel::from(headings)));
        window.show().unwrap();

        let mut boxes = Vec::with_capacity(count);
        for index in 0..count {
            let label = format!("doc-block-{index}");
            let element = i_slint_backend_testing::ElementQuery::from_root(&window)
                .match_descendants()
                .match_predicate(move |element| {
                    element.accessible_label().is_some_and(|text| text == label)
                })
                .find_first()
                .unwrap();
            let pos = element.absolute_position();
            let size = element.size();
            assert!(
                size.height >= 16.0,
                "block {index} collapsed vertically: {size:?}"
            );
            boxes.push((pos.y, pos.y + size.height));
        }

        for pair in boxes.windows(2) {
            assert!(
                pair[1].0 >= pair[0].1 - 0.5,
                "Markdown blocks overlapped: prev_bottom={} next_top={}",
                pair[0].1,
                pair[1].0
            );
        }
    }

    fn find_by_label(
        window: &MainWindow,
        label: &str,
    ) -> Option<i_slint_backend_testing::ElementHandle> {
        let label = label.to_owned();
        i_slint_backend_testing::ElementQuery::from_root(window)
            .match_descendants()
            .match_predicate(move |element| {
                element.accessible_label().is_some_and(|text| text == label)
            })
            .find_first()
    }

    fn settle() {
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
    }

    #[test]
    fn index_and_search_scroll_the_target_block_into_view() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1180.0, 820.0));
        window.set_selected_path("C:/sample/NOTAS.md".into());
        window.set_current_file_path("NOTAS.md".into());
        window.set_markdown_file(true);
        window.set_preview(true);

        let syntax = peek_syntax::ExtensionRegistry::new();
        let md: String = (0..60)
            .map(|i| format!("## Sección {i}\n\nPárrafo número {i} con algo de texto.\n\n"))
            .collect();
        let mut doc_state = crate::doc_view::DocViewState::default();
        doc_state.ensure_markdown_parsed(&md, 1, None, None, &syntax);
        let prepared = doc_state.prepare_models(crate::doc_view::DocLayout {
            width: 900.0,
            font: 15.0,
            zoom: 1.0,
        });
        let (blocks, headings) = prepared.into_slint(false);
        window.set_doc_blocks(ModelRc::new(VecModel::from(blocks)));
        window.set_doc_headings(ModelRc::new(VecModel::from(headings)));
        window.show().unwrap();
        settle();

        // Off-screen blocks are clipped away: block 100 only exists for the query once revealed.
        assert!(find_by_label(&window, "doc-block-0").is_some());
        assert!(find_by_label(&window, "doc-block-100").is_none());

        window.set_doc_scroll_target(100);
        window.set_doc_scroll_seq(window.get_doc_scroll_seq() + 1);
        settle();
        let revealed = find_by_label(&window, "doc-block-100")
            .expect("block 100 was not scrolled into view")
            .absolute_position()
            .y;
        assert!(
            (0.0..400.0).contains(&revealed),
            "block 100 at y={revealed}"
        );
        assert!(find_by_label(&window, "doc-block-0").is_none());

        // A second jump to another block works too, and rebuilding the model does not jump back.
        window.set_doc_scroll_target(4);
        window.set_doc_scroll_seq(window.get_doc_scroll_seq() + 1);
        settle();
        assert!(find_by_label(&window, "doc-block-4").is_some());
        assert!(find_by_label(&window, "doc-block-100").is_none());
    }

    #[test]
    fn panels_collapse_from_the_header_and_documents_render_in_their_modes() {
        i_slint_backend_testing::init_no_event_loop();
        let window = MainWindow::new().unwrap();
        window
            .window()
            .set_size(slint::LogicalSize::new(1320.0, 820.0));
        window.set_busy(false);
        window.show().unwrap();

        // Both panels can be hidden and reopened from the header.
        for (hide, show) in [
            (
                "Ocultar la lista de proyectos (Ctrl+Shift+B)",
                "Mostrar la lista de proyectos (Ctrl+Shift+B)",
            ),
            (
                "Ocultar el explorador (Ctrl+B)",
                "Mostrar el explorador (Ctrl+B)",
            ),
        ] {
            let button = find_by_label(&window, hide).unwrap();
            assert!(
                button.absolute_position().y < 60.0,
                "{hide} is not in the header"
            );
            button.invoke_accessible_default_action();
            settle();
            let reopen = find_by_label(&window, show).unwrap();
            assert!(reopen.absolute_position().y < 60.0);
        }
        assert!(!window.get_show_library() && !window.get_show_tree());
        find_by_label(&window, "Mostrar el explorador (Ctrl+B)")
            .unwrap()
            .invoke_accessible_default_action();
        settle();
        assert!(window.get_show_tree());

        // PDF: page laid out at its logical size with the thumbnail strip on the left.
        let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(8, 8);
        window.set_selected_path("C:/sample/manual.pdf".into());
        window.set_is_pdf(true);
        window.set_pdf_total_pages(3);
        window.set_pdf_page_width(600.0);
        window.set_pdf_page_height(400.0);
        window.set_pdf_image(slint::Image::from_rgba8(pixels.clone()));
        window.set_pdf_thumb_height(88.0);
        window.set_pdf_thumbs(ModelRc::new(VecModel::from(
            (0..3)
                .map(|page| PdfThumbItem {
                    page,
                    has_image: true,
                    image: slint::Image::from_rgba8(pixels.clone()),
                })
                .collect::<Vec<_>>(),
        )));
        settle();
        let first = find_by_label(&window, "Ir a la página 1").unwrap();
        let second = find_by_label(&window, "Ir a la página 2").unwrap();
        assert!(second.absolute_position().y > first.absolute_position().y);
        assert!(first.size().width <= 164.0);
        assert!(find_by_label(&window, "Ocultar miniaturas de página").is_some());
        find_by_label(&window, "Ocultar miniaturas de página")
            .unwrap()
            .invoke_accessible_default_action();
        settle();
        assert!(find_by_label(&window, "Ir a la página 1").is_none());

        // Spreadsheet: the selected cell is readable in full above the grid.
        window.set_is_pdf(false);
        window.set_selected_path("C:/sample/datos.xlsx".into());
        window.set_is_sheet(true);
        window.set_preview(true);
        window.set_sheet_headers(ModelRc::new(VecModel::from(vec![
            SheetHeaderItem {
                label: "A".into(),
                col_index: 0,
                width: 60,
            },
            SheetHeaderItem {
                label: "B".into(),
                col_index: 1,
                width: 240,
            },
        ])));
        window.set_sheet_rows(ModelRc::new(VecModel::from(vec![SheetRowItem {
            row_number: 1,
            cells: ModelRc::new(VecModel::from(vec![
                SheetCellItem {
                    text: "Año".into(),
                    kind: 1,
                    col_index: 0,
                    selected: true,
                    matched: false,
                },
                SheetCellItem {
                    text: "Descripción larga de la canción".into(),
                    kind: 1,
                    col_index: 1,
                    selected: false,
                    matched: false,
                },
            ])),
        }])));
        window.set_sheet_cell_ref("A1".into());
        window.set_sheet_cell_text("Año".into());
        settle();
        assert!(find_by_label(&window, "Celda seleccionada").is_some());
        assert!(find_by_label(&window, "Contenido de la celda").is_some());
    }
}
