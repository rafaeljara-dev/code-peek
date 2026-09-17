mod controller;
mod theme;
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
    connect!(on_revert, || Command::Revert);
    connect!(on_reload, || Command::Reload);
    connect!(on_edit_line, |n| Command::Edit(n.max(1) as usize - 1));
    connect!(on_select_line, |n| Command::Select(n.max(1) as usize - 1));
    connect!(on_apply_edit, |text| Command::Apply(text.to_string()));
    connect!(on_resolve_pending, |choice| Command::Resolve(choice));
    connect!(on_scroll_lines, |n| Command::Scroll(n));
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
    connect!(on_find_text, |text, forward| Command::Find(
        text.to_string(),
        forward
    ));
    connect!(on_cancel_edit, || Command::CancelEdit);
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
            },
            SearchItem {
                path: "two".into(),
                display_path: "two".into(),
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
}
