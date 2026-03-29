//! Inline rename UI — wraps TextInputWidget for F2 rename operations.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::{Renderer, TextEngine};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{EventResponse, Widget, WidgetEvent};
use hayate_ui::widget::toast::ToastLevel;
use hayate_ui::widget::TextInputWidget;

/// Active rename session.
pub(crate) struct RenameState {
    pub(crate) target_path: PathBuf,
    pub(crate) entry_idx: usize,
    widget: TextInputWidget,
}

/// Result of processing an event while renaming.
pub(crate) enum RenameEvent {
    /// Still editing — event was consumed.
    Editing,
    /// User pressed Enter — contains the new name.
    Submit(String),
    /// User pressed Escape — cancel rename.
    Cancel,
}

impl RenameState {
    /// Start a rename session for the given file/directory.
    pub(crate) fn new(
        target_path: PathBuf,
        original_name: &str,
        entry_idx: usize,
        engine: Rc<RefCell<TextEngine>>,
        width: f32,
    ) -> Self {
        let mut widget = TextInputWidget::new(engine).with_width(width);
        // Pre-fill with the current name, select all for easy replacement
        widget.input_mut().insert_str(original_name);
        widget.input_mut().select_all();
        widget.input_mut().set_focused(true);
        Self { target_path, entry_idx, widget }
    }

    /// Route a WidgetEvent into the rename text input.
    ///
    /// Returns a `RenameEvent` indicating whether the session is still active,
    /// was submitted, or was cancelled.
    pub(crate) fn handle_event(&mut self, event: &WidgetEvent) -> RenameEvent {
        // Intercept Escape before forwarding to the widget
        if let WidgetEvent::Key(ke) = event {
            if ke.keysym == xkbcommon::xkb::Keysym::Escape {
                return RenameEvent::Cancel;
            }
        }
        let resp = self.widget.event(event);
        // Check if TextInputWidget signalled Submit (Enter)
        if resp == EventResponse::Handled {
            if let WidgetEvent::Key(ke) = event {
                if ke.keysym == xkbcommon::xkb::Keysym::Return
                    || ke.keysym == xkbcommon::xkb::Keysym::KP_Enter
                {
                    let new_name = self.widget.text().to_owned();
                    return RenameEvent::Submit(new_name);
                }
            }
        }
        RenameEvent::Editing
    }

    /// Paint the text input overlay at the given rect.
    pub(crate) fn paint(&self, renderer: &mut Renderer, rect: ItemRect) {
        self.widget.paint(renderer, rect);
    }
}

/// Start a rename session on the given FileListWidget.
///
/// Called from keybindings (F2) and context menu (Rename action).
pub(crate) fn start_rename(
    w: &mut crate::file_list::FileListWidget,
    idx: usize,
    name: &str,
) {
    let state = w.state.borrow();
    let path = state.current_path.join(name);
    let engine = state.engine.clone();
    let width = w.width;
    drop(state);
    w.rename_state = Some(RenameState::new(path, name, idx, engine, width));
}

/// Process an event while FileListWidget is in rename mode.
///
/// Called from `FileListWidget::handle_event_inner` when `rename_state.is_some()`.
pub(crate) fn handle_rename_event(
    w: &mut crate::file_list::FileListWidget,
    event: &WidgetEvent,
) -> EventResponse {
    // Only forward key and IME events to the rename widget
    let forward = matches!(
        event,
        WidgetEvent::Key(_)
            | WidgetEvent::ImeCommit(_)
            | WidgetEvent::ImePreedit { .. }
            | WidgetEvent::ImePreeditClear
    );
    if !forward {
        return EventResponse::Ignored;
    }
    let rs = w.rename_state.as_mut().unwrap();
    match rs.handle_event(event) {
        RenameEvent::Editing => EventResponse::Handled,
        RenameEvent::Submit(new_name) => {
            let target = rs.target_path.clone();
            w.rename_state = None;
            if !new_name.is_empty() && !new_name.contains('/') && !new_name.contains('\0') {
                match crate::file_ops::rename_file(&target, &new_name) {
                    Ok(p) => {
                        eprintln!("[rename] Renamed to: {}", p.display());
                        w.toast.borrow_mut().show(format!("Renamed to {new_name}"), ToastLevel::Success, 2.0);
                    }
                    Err(e) => {
                        eprintln!("[rename] Failed: {e}");
                        w.toast.borrow_mut().show(format!("Rename failed: {e}"), ToastLevel::Error, 5.0);
                    }
                }
                w.state.borrow_mut().refresh();
                w.refresh_viewport();
            }
            EventResponse::Handled
        }
        RenameEvent::Cancel => {
            w.rename_state = None;
            EventResponse::Handled
        }
    }
}
