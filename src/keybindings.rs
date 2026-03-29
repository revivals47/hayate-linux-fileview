//! Keyboard event handling extracted from FileListWidget.

use std::path::PathBuf;
use std::time::Instant;

use xkbcommon::xkb::Keysym;

use hayate_ui::widget::core::EventResponse;
use hayate_ui::widget::toast::ToastLevel;
use hayate_ui::platform::keyboard::KeyEvent;

use crate::entry::SortColumn;
use crate::file_list::{FileListWidget, JUMP_TIMEOUT_MS};

pub(crate) fn handle_key_event(w: &mut FileListWidget, ke: &KeyEvent) -> EventResponse {
    // Ctrl+Q → save config and graceful quit via quit_flag
    if ke.modifiers.ctrl && ke.keysym == Keysym::q {
        let state = w.state.borrow();
        let cfg = crate::config::Config::from_state(&state);
        cfg.save();
        if let Some(ref qf) = state.quit_flag {
            qf.set(true);
        }
        return EventResponse::Handled;
    }

    // ── Search mode ──
    if w.search_mode {
        match ke.keysym {
            Keysym::Escape => {
                w.search_mode = false;
                w.state.borrow_mut().set_search(None);
                w.refresh_viewport();
            }
            Keysym::Return => { w.search_mode = false; }
            Keysym::BackSpace => {
                let mut st = w.state.borrow_mut();
                if let Some(ref mut q) = st.search_query { q.pop(); }
                st.update_filter();
                drop(st);
                w.refresh_viewport();
            }
            _ => {
                if let Some(ref text) = ke.utf8 {
                    let mut st = w.state.borrow_mut();
                    for ch in text.chars().filter(|c| !c.is_control()) {
                        if let Some(ref mut q) = st.search_query { q.push(ch); }
                    }
                    st.update_filter();
                    if let Some(ref fi) = st.filtered_indices {
                        if let Some(&first) = fi.first() { st.select_single(first); }
                    }
                    drop(st);
                    w.refresh_viewport();
                    w.ensure_cursor_visible();
                }
            }
        }
        return EventResponse::Handled;
    }

    if ke.modifiers.ctrl && ke.keysym == Keysym::f {
        w.search_mode = true;
        w.state.borrow_mut().set_search(Some(String::new()));
        return EventResponse::Handled;
    }
    if ke.keysym == Keysym::Escape && w.state.borrow().search_query.is_some() {
        w.state.borrow_mut().set_search(None);
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    if ke.modifiers.ctrl && ke.keysym == Keysym::h {
        w.state.borrow_mut().toggle_hidden();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    if ke.keysym == Keysym::BackSpace {
        w.state.borrow_mut().go_parent();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    // Alt+Left → navigate back
    if ke.modifiers.alt && ke.keysym == Keysym::Left {
        w.state.borrow_mut().go_back();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    // Alt+Right → navigate forward
    if ke.modifiers.alt && ke.keysym == Keysym::Right {
        w.state.borrow_mut().go_forward();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    if ke.keysym == Keysym::Up || ke.keysym == Keysym::Down {
        let mut state = w.state.borrow_mut();
        let count = state.entries.len();
        if count > 0 {
            let new_idx = match (state.cursor, ke.keysym) {
                (None, Keysym::Down) => 0,
                (None, _) => count - 1,
                (Some(cur), Keysym::Down) => (cur + 1).min(count - 1),
                (Some(cur), _) => cur.saturating_sub(1),
            };
            if ke.modifiers.shift {
                let anchor = state.anchor.unwrap_or(new_idx);
                state.select_range(anchor, new_idx);
            } else {
                state.select_single(new_idx);
            }
            drop(state);
            w.ensure_cursor_visible();
        }
        return EventResponse::Handled;
    }
    if ke.modifiers.ctrl && ke.keysym == Keysym::a {
        w.state.borrow_mut().select_all();
        return EventResponse::Handled;
    }
    // Tab → cycle view mode (Detail → List → Compact)
    if ke.keysym == Keysym::Tab {
        w.state.borrow_mut().cycle_view_mode();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    if ke.keysym == Keysym::Return {
        let state = w.state.borrow();
        if let Some(idx) = state.cursor {
            if idx < state.entries.len() {
                let path = state.current_path.join(&state.entries[idx].name);
                if state.entries[idx].is_dir {
                    drop(state);
                    w.state.borrow_mut().navigate(path);
                    w.refresh_viewport();
                } else {
                    drop(state);
                    crate::file_list::open_with_xdg(&path);
                }
                return EventResponse::Handled;
            }
        }
        return EventResponse::Ignored;
    }
    if ke.modifiers.ctrl {
        let col = match ke.keysym {
            Keysym::_1 => Some(SortColumn::Name),
            Keysym::_2 => Some(SortColumn::Size),
            Keysym::_3 => Some(SortColumn::Modified),
            _ => None,
        };
        if let Some(c) = col {
            w.state.borrow_mut().set_sort(c);
            w.refresh_viewport();
            return EventResponse::Handled;
        }
    }
    // Debounce file operations (prevent key-repeat double execution)
    let now = Instant::now();
    let debounce_ok = w.last_file_op
        .map(|t| now.duration_since(t).as_millis() > 300)
        .unwrap_or(true);

    if ke.modifiers.ctrl && ke.keysym == Keysym::c && debounce_ok {
        w.last_file_op = Some(now);
        let state = w.state.borrow();
        let paths: Vec<PathBuf> = state.selected_indices().iter()
            .filter(|&&i| i < state.entries.len())
            .map(|&i| state.current_path.join(&state.entries[i].name))
            .collect();
        let count = paths.len();
        // Write to system clipboard as text/uri-list
        if let Some(ref buf) = state.system_clipboard {
            let uri_list = paths.iter()
                .map(|p| format!("file://{}", p.display()))
                .collect::<Vec<_>>()
                .join("\r\n");
            buf.borrow_mut().replace(uri_list);
        }
        drop(state);
        w.clipboard = paths;
        eprintln!("[file_ops] Copied {} file(s) to clipboard", count);
        w.toast.borrow_mut().show(format!("Copied {count} file(s)"), ToastLevel::Info, 2.0);
        return EventResponse::Handled;
    }
    if ke.modifiers.ctrl && ke.keysym == Keysym::v && debounce_ok {
        w.last_file_op = Some(now);
        if !w.clipboard.is_empty() {
            // Internal buffer has files — paste directly
            let dest = w.state.borrow().current_path.clone();
            let mut ok = 0usize;
            for src in &w.clipboard {
                match crate::file_ops::copy_to(src, &dest) {
                    Ok(_) => ok += 1,
                    Err(e) => eprintln!("[file_ops] copy error: {}: {}", src.display(), e),
                }
            }
            let total = w.clipboard.len();
            eprintln!("[file_ops] Pasted {ok}/{total} file(s)");
            w.toast.borrow_mut().show(format!("Pasted {ok}/{total} file(s)"), ToastLevel::Success, 3.0);
            w.state.borrow_mut().refresh();
            w.refresh_viewport();
        } else if let Some(ref pr) = w.state.borrow().paste_request {
            // Internal buffer empty — request paste from system clipboard
            pr.set(true);
            w.pending_file_paste = true;
        }
        return EventResponse::Handled;
    }
    if ke.keysym == Keysym::Delete && debounce_ok {
        w.last_file_op = Some(now);
        let state = w.state.borrow();
        let paths: Vec<PathBuf> = state.selected_indices().iter()
            .filter(|&&i| i < state.entries.len())
            .map(|&i| state.current_path.join(&state.entries[i].name))
            .collect();
        drop(state);
        if paths.is_empty() {
            eprintln!("[file_ops] Delete: no files selected");
        } else {
            let mut ok = 0usize;
            for p in &paths {
                match crate::file_ops::trash(p) {
                    Ok(()) => ok += 1,
                    Err(e) => eprintln!("[file_ops] trash error: {}: {}", p.display(), e),
                }
            }
            let total = paths.len();
            eprintln!("[file_ops] Trashed {ok}/{total} file(s)");
            w.toast.borrow_mut().show(format!("Moved {ok} file(s) to trash"), ToastLevel::Info, 3.0);
            w.state.borrow_mut().refresh();
            w.refresh_viewport();
        }
        return EventResponse::Handled;
    }
    // F2 → start inline rename
    if ke.keysym == Keysym::F2 && debounce_ok {
        w.last_file_op = Some(now);
        let state = w.state.borrow();
        if let Some(idx) = state.cursor {
            if idx < state.entries.len() {
                let name = state.entries[idx].name.clone();
                drop(state);
                crate::rename_ui::start_rename(w, idx, &name);
            }
        }
        return EventResponse::Handled;
    }
    // Ctrl+Shift+N → create new folder
    if ke.modifiers.ctrl && ke.modifiers.shift
        && (ke.keysym == Keysym::n || ke.keysym == Keysym::N)
        && debounce_ok
    {
        w.last_file_op = Some(now);
        let dir = w.state.borrow().current_path.clone();
        match crate::file_ops::create_directory(&dir) {
            Ok(p) => {
                eprintln!("[file_ops] Created: {}", p.display());
                w.toast.borrow_mut().show("Created folder", ToastLevel::Success, 2.0);
            }
            Err(e) => {
                eprintln!("[file_ops] Create folder failed: {e}");
                w.toast.borrow_mut().show(format!("Create folder failed: {e}"), ToastLevel::Error, 5.0);
            }
        }
        w.state.borrow_mut().refresh();
        w.refresh_viewport();
        return EventResponse::Handled;
    }
    // Scroll keys
    let delta = match ke.keysym {
        Keysym::Page_Up => Some(-w.height * 0.8),
        Keysym::Page_Down => Some(w.height * 0.8),
        Keysym::Home => Some(-w.viewport.scroll_offset()),
        Keysym::End => Some(w.viewport.content_height()),
        _ => None,
    };
    if let Some(d) = delta {
        w.viewport.scroll(d, 1.0 / 60.0);
        return EventResponse::Handled;
    }
    // Incremental jump
    if !ke.modifiers.ctrl && !ke.modifiers.alt {
        if let Some(ref text) = ke.utf8 {
            for ch in text.chars() {
                if ch.is_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                    let now = Instant::now();
                    if let Some(last) = w.jump_last_input {
                        if now.duration_since(last).as_millis() > JUMP_TIMEOUT_MS {
                            w.jump_buffer.clear();
                        }
                    }
                    w.jump_buffer.push(ch);
                    w.jump_last_input = Some(now);
                    w.jump_to_prefix();
                    return EventResponse::Handled;
                }
            }
        }
    }
    EventResponse::Ignored
}

/// Process system clipboard paste result (text/uri-list).
pub(crate) fn handle_clipboard_paste(w: &mut FileListWidget, text: &str) {
    let paths = crate::file_ops::parse_uri_list(text);
    if paths.is_empty() { return; }
    let dest = w.state.borrow().current_path.clone();
    let mut ok = 0usize;
    for src in &paths {
        if crate::file_ops::copy_to(src, &dest).is_ok() { ok += 1; }
    }
    let total = paths.len();
    eprintln!("[paste] Pasted {ok}/{total} file(s) from system clipboard");
    w.toast.borrow_mut().show(format!("Pasted {ok}/{total} file(s)"), ToastLevel::Success, 3.0);
    w.state.borrow_mut().refresh();
    w.refresh_viewport();
}
