mod breadcrumb;
mod config;
mod entry;
mod file_list;
mod file_ops;
mod keybindings;
mod preview;
mod scroll;
mod sidebar;
mod state;
mod status_bar;
mod three_pane;

use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::app::App;
use hayate_ui::render::TextEngine;
use hayate_ui::widget::core::Widget;

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

    let root: Box<dyn Widget> = Box::new(ThreePaneWidget::new(state, engine));

    let title = format!("Hayate — {}", path.display());
    if let Err(e) = App::new(title, cfg.window_width, cfg.window_height).run(root) {
        eprintln!("Error: {e}");
    }
}
