mod controller;
mod editor;
pub mod image_loader;
mod theme;
mod thumbnails;
pub mod viewport;
mod wrapping;

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
                        "Buscar y reemplazar (Ctrl+H)",
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
}
