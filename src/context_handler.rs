//! Right-click context menu: item definitions, action dispatch, and label painting.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::{Renderer, TextEngine};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::context_menu::{ContextMenu, MenuItem};
use hayate_ui::widget::core::Widget;

use hayate_ui::widget::toast::ToastLevel;
use crate::file_list::FileListWidget;

// ── Menu construction ──

pub(crate) fn build_menu() -> ContextMenu {
    ContextMenu::new(vec![
        MenuItem::new("open", "Open"),
        MenuItem::new("copy", "Copy"),
        MenuItem::new("paste", "Paste"),
        MenuItem::new("delete", "Delete"),
        MenuItem::new("rename", "Rename"),
        MenuItem::new("new_folder", "New Folder"),
    ])
}

// ── Show menu with entry selection ──

pub(crate) fn show_at(w: &mut FileListWidget, x: f32, y: f32) {
    // Select the entry under the cursor (if any) before showing menu
    if let Some(crate::file_list::YHit::Entry(idx)) = w.y_x_to_hit(y, x) {
        let mut state = w.state.borrow_mut();
        if idx < state.entries.len() && !state.is_selected(idx) {
            state.select_single(idx);
        }
    }
    w.context_menu.show(x, y);
}

// ── Dispatch selected action ──

pub(crate) fn dispatch(w: &mut FileListWidget, action_id: &str) {
    match action_id {
        "open" => action_open(w),
        "copy" => action_copy(w),
        "paste" => action_paste(w),
        "delete" => action_delete(w),
        "rename" => action_rename(w),
        "new_folder" => action_new_folder(w),
        _ => {}
    }
}

fn action_open(w: &mut FileListWidget) {
    let state = w.state.borrow();
    if let Some(idx) = state.cursor
        && idx < state.entries.len() {
            let path = state.current_path.join(&state.entries[idx].name);
            if state.entries[idx].is_dir {
                drop(state);
                w.state.borrow_mut().navigate(path);
                w.refresh_viewport();
            } else {
                drop(state);
                crate::file_list::open_with_xdg(&path);
            }
        }
}

fn action_copy(w: &mut FileListWidget) {
    let state = w.state.borrow();
    let paths: Vec<PathBuf> = state.selected_indices().iter()
        .filter(|&&i| i < state.entries.len())
        .map(|&i| state.current_path.join(&state.entries[i].name))
        .collect();
    let count = paths.len();
    if let Some(ref buf) = state.system_clipboard {
        let uri_list = paths.iter()
            .map(|p| format!("file://{}", p.display()))
            .collect::<Vec<_>>()
            .join("\r\n");
        buf.borrow_mut().replace(uri_list);
    }
    drop(state);
    w.clipboard = paths;
    eprintln!("[ctx] Copied {count} file(s) to clipboard");
    w.toast.borrow_mut().show(format!("Copied {count} file(s)"), ToastLevel::Info, 2.0);
}

fn action_paste(w: &mut FileListWidget) {
    if !w.clipboard.is_empty() {
        let dest = w.state.borrow().current_path.clone();
        let mut ok = 0usize;
        for src in &w.clipboard {
            if crate::file_ops::copy_to(src, &dest).is_ok() { ok += 1; }
        }
        let total = w.clipboard.len();
        eprintln!("[ctx] Pasted {ok}/{total} file(s)");
        w.toast.borrow_mut().show(format!("Pasted {ok}/{total} file(s)"), ToastLevel::Success, 3.0);
        w.state.borrow_mut().refresh();
        w.refresh_viewport();
    } else if let Some(ref pr) = w.state.borrow().paste_request {
        pr.set(true);
        w.pending_file_paste = true;
    }
}

fn action_delete(w: &mut FileListWidget) {
    let state = w.state.borrow();
    let paths: Vec<PathBuf> = state.selected_indices().iter()
        .filter(|&&i| i < state.entries.len())
        .map(|&i| state.current_path.join(&state.entries[i].name))
        .collect();
    drop(state);
    if paths.is_empty() { return; }
    let mut ok = 0usize;
    for p in &paths {
        if crate::file_ops::trash(p).is_ok() { ok += 1; }
    }
    let total = paths.len();
    eprintln!("[ctx] Trashed {ok}/{total} file(s)");
    w.toast.borrow_mut().show(format!("Moved {ok} file(s) to trash"), ToastLevel::Info, 3.0);
    w.state.borrow_mut().refresh();
    w.refresh_viewport();
}

fn action_rename(w: &mut FileListWidget) {
    let state = w.state.borrow();
    if let Some(idx) = state.cursor
        && idx < state.entries.len() {
            let name = state.entries[idx].name.clone();
            drop(state);
            crate::rename_ui::start_rename(w, idx, &name);
        }
}

fn action_new_folder(w: &mut FileListWidget) {
    let dir = w.state.borrow().current_path.clone();
    match crate::file_ops::create_directory(&dir) {
        Ok(p) => {
            eprintln!("[ctx] Created: {}", p.display());
            w.toast.borrow_mut().show("Created folder", ToastLevel::Success, 2.0);
        }
        Err(e) => {
            eprintln!("[ctx] Create folder failed: {e}");
            w.toast.borrow_mut().show(format!("Create folder failed: {e}"), ToastLevel::Error, 5.0);
        }
    }
    w.state.borrow_mut().refresh();
    w.refresh_viewport();
}

// ── Paint menu text labels ──

pub(crate) fn paint_menu(
    menu: &mut ContextMenu,
    engine: &Rc<RefCell<TextEngine>>,
    renderer: &mut Renderer,
    rect: ItemRect,
) {
    if !menu.is_visible() { return; }
    menu.inject_engine(engine.clone());
    menu.paint(renderer, rect);
}
