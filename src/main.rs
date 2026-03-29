mod breadcrumb;
mod config;
mod context_handler;
mod entry;
mod file_list;
mod file_ops;
mod keybindings;
mod lru_cache;
mod preview;
mod rename_ui;
mod sidebar;
mod state;
mod status_bar;
mod tab_bar;
mod terminal_pty;
mod terminal_state;
mod terminal_widget;
mod three_pane;
mod watcher;

use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::app::App;
use hayate_ui::render::TextEngine;
use hayate_ui::widget::core::Widget;
use hayate_ui::widget::{DragZone, VStack};

use state::FileViewState;
use three_pane::ThreePaneWidget;

fn main() {
    let cfg = config::Config::load();

    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    let engine = Rc::new(RefCell::new(TextEngine::new()));
    let state = Rc::new(RefCell::new(FileViewState::new_with_config(
        path.clone(), engine.clone(),
        cfg.to_sort_column(), cfg.to_sort_order(),
        cfg.show_hidden, cfg.to_view_mode(),
    )));

    let title = format!("Hayate — {}", path.display());
    let app = App::new(title, cfg.window_width, cfg.window_height)
        .with_min_size(400, 300);
    let quit_flag = app.quit_flag();
    let title_buf = app.title_buffer();
    let clipboard_buf = app.clipboard_copy_buffer();
    let paste_req = app.clipboard_paste_request();

    {
        let mut s = state.borrow_mut();
        s.quit_flag = Some(quit_flag);
        s.title_buffer = Some(title_buf);
        s.system_clipboard = Some(clipboard_buf);
        s.paste_request = Some(paste_req);
        s.sidebar_ratio.set(cfg.sidebar_ratio);
        s.preview_ratio.set(cfg.preview_ratio);
    }

    let mut three_pane = ThreePaneWidget::new(state, engine);
    three_pane.set_cursor_shape_buffer(app.cursor_shape_buffer());
    let move_req = app.move_request();
    let root: Box<dyn Widget> = Box::new(
        VStack::new(0.0)
            .add(Box::new(DragZone::new(move_req)))
            .add(Box::new(three_pane))
    );

    if let Err(e) = app.run(root) {
        eprintln!("Error: {e}");
    }
}
