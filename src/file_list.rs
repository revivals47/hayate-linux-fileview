//! File list widget with event handling, column headers, and sort support.

use std::cell::RefCell;
use std::rc::Rc;

use xkbcommon::xkb::Keysym;

use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_ui::widget::layout::{Padding, VStack};
use hayate_ui::widget::text_widget::RichTextWidget;

use crate::entry::SortColumn;
use crate::scroll::ScrollableWidget;
use crate::state::FileViewState;

// ── Layout constants ──

/// Header: font 16 + spacing 1 = 17
const HEADER_HEIGHT: f32 = 17.0;
/// Separator: font 8 + spacing 1 = 9
const SEP_HEIGHT: f32 = 9.0;
/// Column header row
const COLUMN_HEADER_HEIGHT: f32 = 14.0;
/// Column header underline
const COLUMN_SEP_HEIGHT: f32 = 9.0;
/// Entry rows (including ".."): font 13 + spacing 1 = 14
const ROW_HEIGHT: f32 = 14.0;
/// Padding around the entire list
const LIST_PADDING: f32 = 12.0;

// ── Widget tree builder ──

fn build_file_list(state: &FileViewState) -> Box<dyn Widget> {
    let engine = &state.engine;
    let mut vstack = VStack::new(1.0);

    // Header: current path + hidden indicator
    let hidden_indicator = if state.show_hidden { " [H]" } else { "" };
    let header = RichTextWidget::new(
        format!("  {}{}", state.current_path.display(), hidden_indicator),
        16.0,
    )
    .with_engine(engine.clone())
    .with_color(100, 180, 255);
    vstack.push(Box::new(header));

    // Separator
    let sep = RichTextWidget::new("─".repeat(80), 8.0)
        .with_engine(engine.clone())
        .with_color(60, 60, 60);
    vstack.push(Box::new(sep));

    // Column header with sort indicator
    let ind = state.sort_order.indicator();
    let name_ind = if state.sort_column == SortColumn::Name { ind } else { " " };
    let size_ind = if state.sort_column == SortColumn::Size { ind } else { " " };
    let mod_ind = if state.sort_column == SortColumn::Modified { ind } else { " " };
    let col_header = RichTextWidget::new(
        format!(
            "    Perm      Name {}{:<27} {:>10}{}  {}{}",
            name_ind, "", "Size", size_ind, "Modified", mod_ind
        ),
        13.0,
    )
    .with_engine(engine.clone())
    .with_color(120, 120, 120);
    vstack.push(Box::new(col_header));

    // Column underline
    let col_sep = RichTextWidget::new("─".repeat(75), 8.0)
        .with_engine(engine.clone())
        .with_color(60, 60, 60);
    vstack.push(Box::new(col_sep));

    // Parent directory entry (..)
    let parent_row = RichTextWidget::new("📁  ../".to_string(), 13.0)
        .with_engine(engine.clone())
        .with_color(150, 150, 220);
    vstack.push(Box::new(parent_row));

    // File/directory entries
    for (i, entry) in state.entries.iter().enumerate() {
        let selected = state.selected_index == Some(i);
        let (r, g, b) = if selected {
            (80, 200, 255)
        } else if entry.is_dir {
            (220, 180, 80)
        } else {
            (190, 190, 190)
        };
        let line = if selected {
            format!("▶ {}", entry.display_line().trim_start())
        } else {
            entry.display_line()
        };
        let w = RichTextWidget::new(line, 13.0)
            .with_engine(engine.clone())
            .with_color(r, g, b);
        vstack.push(Box::new(w));
    }

    // Footer
    let footer = RichTextWidget::new(format!("  {} items", state.entries.len()), 11.0)
        .with_engine(engine.clone())
        .with_color(100, 100, 100);
    vstack.push(Box::new(footer));

    Box::new(Padding::all(LIST_PADDING, Box::new(vstack)))
}

// ── FileListWidget ──

pub(crate) struct FileListWidget {
    state: Rc<RefCell<FileViewState>>,
    scroller: ScrollableWidget,
}

impl FileListWidget {
    pub(crate) fn new(state: Rc<RefCell<FileViewState>>) -> Self {
        let inner = build_file_list(&state.borrow());
        let scroller = ScrollableWidget::new(inner, 16.0);
        Self { state, scroller }
    }

    fn rebuild(&mut self) {
        let inner = build_file_list(&self.state.borrow());
        self.scroller.inner = inner;
        self.scroller.reset_scroll();
    }

    fn rebuild_keep_scroll(&mut self) {
        let inner = build_file_list(&self.state.borrow());
        self.scroller.inner = inner;
    }

    fn ensure_selected_visible(&mut self) {
        let idx = match self.state.borrow().selected_index {
            Some(i) => i,
            None => return,
        };
        let row = idx + 1;
        let row_top = LIST_PADDING
            + HEADER_HEIGHT
            + SEP_HEIGHT
            + COLUMN_HEADER_HEIGHT
            + COLUMN_SEP_HEIGHT
            + row as f32 * ROW_HEIGHT;
        let row_bottom = row_top + ROW_HEIGHT;

        if row_top < self.scroller.scroll_offset {
            self.scroller.scroll_offset = row_top;
        } else if row_bottom > self.scroller.scroll_offset + self.scroller.viewport_height {
            self.scroller.scroll_offset = row_bottom - self.scroller.viewport_height;
        }
    }

    fn is_column_header_y(&self, y: f32) -> bool {
        let col_start = LIST_PADDING + HEADER_HEIGHT + SEP_HEIGHT;
        y >= col_start && y < col_start + COLUMN_HEADER_HEIGHT
    }

    fn x_to_sort_column(&self, x: f32) -> SortColumn {
        let char_x = ((x - LIST_PADDING) / 7.0) as usize;
        if char_x >= 54 {
            SortColumn::Modified
        } else if char_x >= 44 {
            SortColumn::Size
        } else {
            SortColumn::Name
        }
    }

    fn y_to_row(&self, y: f32) -> Option<usize> {
        let entries_start =
            LIST_PADDING + HEADER_HEIGHT + SEP_HEIGHT + COLUMN_HEADER_HEIGHT + COLUMN_SEP_HEIGHT;
        if y < entries_start {
            return None;
        }
        let row = ((y - entries_start) / ROW_HEIGHT) as usize;
        if row <= self.state.borrow().entries.len() {
            Some(row)
        } else {
            None
        }
    }
}

impl Widget for FileListWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.scroller.layout(constraints)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        self.scroller.paint(canvas, rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            // Left-click (BTN_LEFT = 0x110 in evdev)
            WidgetEvent::PointerPress { x, y, button: 0x110 } => {
                let content_y = *y + self.scroller.scroll_offset;
                // Column header click → toggle sort
                if self.is_column_header_y(content_y) {
                    let col = self.x_to_sort_column(*x);
                    self.state.borrow_mut().set_sort(col);
                    self.rebuild();
                    return EventResponse::Handled;
                }
                if let Some(row) = self.y_to_row(content_y) {
                    let mut state = self.state.borrow_mut();
                    if row == 0 {
                        state.go_parent();
                        drop(state);
                        self.rebuild();
                    } else {
                        let idx = row - 1;
                        if idx < state.entries.len() && state.entries[idx].is_dir {
                            let new_path = state.current_path.join(&state.entries[idx].name);
                            state.navigate(new_path);
                            drop(state);
                            self.rebuild();
                        } else if idx < state.entries.len() {
                            state.selected_index = Some(idx);
                            drop(state);
                            self.rebuild_keep_scroll();
                        } else {
                            return EventResponse::Ignored;
                        }
                    }
                    return EventResponse::Handled;
                }
                let _ = x;
                EventResponse::Ignored
            }

            // Keyboard events
            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                if ke.modifiers.ctrl && ke.keysym == Keysym::h {
                    self.state.borrow_mut().toggle_hidden();
                    self.rebuild();
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::BackSpace {
                    self.state.borrow_mut().go_parent();
                    self.rebuild();
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::Up || ke.keysym == Keysym::Down {
                    let mut state = self.state.borrow_mut();
                    let count = state.entries.len();
                    if count > 0 {
                        let new_idx = match (state.selected_index, ke.keysym) {
                            (None, Keysym::Down) => 0,
                            (None, _) => count - 1,
                            (Some(cur), Keysym::Down) => (cur + 1).min(count - 1),
                            (Some(cur), _) => cur.saturating_sub(1),
                        };
                        state.selected_index = Some(new_idx);
                        drop(state);
                        self.rebuild_keep_scroll();
                        self.ensure_selected_visible();
                    }
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::Return {
                    let mut state = self.state.borrow_mut();
                    if let Some(idx) = state.selected_index {
                        if idx < state.entries.len() && state.entries[idx].is_dir {
                            let new_path = state.current_path.join(&state.entries[idx].name);
                            state.navigate(new_path);
                            drop(state);
                            self.rebuild();
                            return EventResponse::Handled;
                        }
                    }
                    return EventResponse::Ignored;
                }
                // Ctrl+1/2/3 → sort by Name/Size/Modified
                if ke.modifiers.ctrl {
                    let sort_col = match ke.keysym {
                        Keysym::_1 => Some(SortColumn::Name),
                        Keysym::_2 => Some(SortColumn::Size),
                        Keysym::_3 => Some(SortColumn::Modified),
                        _ => None,
                    };
                    if let Some(col) = sort_col {
                        self.state.borrow_mut().set_sort(col);
                        self.rebuild_keep_scroll();
                        return EventResponse::Handled;
                    }
                }
                if ke.modifiers.ctrl && ke.keysym == Keysym::v {
                    let dest = self.state.borrow().current_path.clone();
                    eprintln!(
                        "[file_ops] Ctrl+V: paste into {} (stub — clipboard not yet available)",
                        dest.display()
                    );
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::Delete {
                    let dir = self.state.borrow().current_path.clone();
                    eprintln!(
                        "[file_ops] Delete: request delete in {} (stub — selection not yet implemented)",
                        dir.display()
                    );
                    return EventResponse::Handled;
                }
                self.scroller.event(event)
            }

            _ => self.scroller.event(event),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        true
    }
}
