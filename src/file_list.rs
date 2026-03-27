//! Virtualized file list widget using VirtualViewport + direct TextEngine rendering.

use std::cell::RefCell;
use std::rc::Rc;

use tiny_skia::Color;
use xkbcommon::xkb::Keysym;

use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::render::{FontFamily, TextEngine, TextParams};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::scroll::physics::PixelScrollPhysics;
use hayate_ui::scroll::viewport::VirtualViewport;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::entry::SortColumn;
use crate::state::FileViewState;

// ── Layout constants ──

const ROW_HEIGHT: f32 = 18.0;
/// Fixed rows: header, separator, column header, column separator, parent (..)
const FIXED_ROWS: usize = 5;
const HEADER_ROW: usize = 0;
const SEP_ROW: usize = 1;
const COL_HEADER_ROW: usize = 2;
const COL_SEP_ROW: usize = 3;
const PARENT_ROW: usize = 4;
const PADDING: f32 = 12.0;

fn color_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

// ── FileListWidget ──

pub(crate) struct FileListWidget {
    state: Rc<RefCell<FileViewState>>,
    viewport: VirtualViewport,
    engine: Rc<RefCell<TextEngine>>,
    width: f32,
    height: f32,
}

impl FileListWidget {
    pub(crate) fn new(state: Rc<RefCell<FileViewState>>) -> Self {
        let engine = state.borrow().engine.clone();
        let entry_count = state.borrow().entries.len();
        let physics = Box::new(PixelScrollPhysics::new());
        let mut viewport = VirtualViewport::new(700.0, ROW_HEIGHT, physics);
        viewport.on_total_changed(FIXED_ROWS + entry_count);
        Self {
            state,
            viewport,
            engine,
            width: 0.0,
            height: 0.0,
        }
    }

    pub(crate) fn state(&self) -> &Rc<RefCell<FileViewState>> {
        &self.state
    }

    /// Refresh after external state changes (e.g. sidebar navigation).
    pub(crate) fn rebuild(&mut self) {
        self.refresh_viewport();
    }

    fn refresh_viewport(&mut self) {
        let count = self.state.borrow().entries.len();
        self.viewport.on_total_changed(FIXED_ROWS + count);
    }

    fn ensure_selected_visible(&mut self) {
        if let Some(idx) = self.state.borrow().selected_index {
            self.viewport.scroll_to_item(FIXED_ROWS + idx);
        }
    }

    /// Map viewport-space Y to a hit target.
    fn y_to_hit(&self, y: f32) -> Option<YHit> {
        let content_y = y + self.viewport.scroll_offset();
        if content_y < 0.0 {
            return None;
        }
        let row = (content_y / ROW_HEIGHT) as usize;
        match row {
            COL_HEADER_ROW => Some(YHit::ColumnHeader),
            PARENT_ROW => Some(YHit::Parent),
            r if r >= FIXED_ROWS => {
                let idx = r - FIXED_ROWS;
                if idx < self.state.borrow().entries.len() {
                    Some(YHit::Entry(idx))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn x_to_sort_column(&self, x: f32) -> SortColumn {
        let char_x = ((x - PADDING) / 7.0) as usize;
        if char_x >= 54 {
            SortColumn::Modified
        } else if char_x >= 44 {
            SortColumn::Size
        } else {
            SortColumn::Name
        }
    }

    fn draw_text_row(
        engine: &mut TextEngine,
        canvas: &mut [u8],
        stride: u32,
        rect: &ItemRect,
        x: f32,
        y: f32,
        text: &str,
        font_size: f32,
        color: Color,
        max_w: f32,
        clip_h: f32,
    ) {
        let params = TextParams {
            text,
            font_size,
            line_height: ROW_HEIGHT,
            color,
            family: FontFamily::Monospace,
        };
        let buffer = engine.layout(&params, max_w);
        engine.draw_buffer(
            &buffer,
            canvas,
            stride,
            (rect.x + x) as i32,
            (rect.y + y) as i32,
            color,
            max_w as u32,
            clip_h as u32,
        );
    }
}

enum YHit {
    ColumnHeader,
    Parent,
    Entry(usize),
}

impl Widget for FileListWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.width = constraints.max_width;
        self.height = constraints.max_height;
        self.viewport.set_viewport_height(constraints.max_height);
        Size::new(constraints.max_width, constraints.max_height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let state = self.state.borrow();
        let mut engine = self.engine.borrow_mut();
        let range = self.viewport.visible_range();
        let max_w = (self.width - PADDING * 2.0).max(0.0);

        for virt_idx in range {
            let y = self.viewport.item_y_in_viewport(virt_idx);
            if y + ROW_HEIGHT < 0.0 || y > self.height {
                continue;
            }

            match virt_idx {
                HEADER_ROW => {
                    let hidden = if state.show_hidden { " [H]" } else { "" };
                    let text = format!("  {}{}", state.current_path.display(), hidden);
                    Self::draw_text_row(
                        &mut engine, canvas, stride, &rect, PADDING, y,
                        &text, 16.0, color_rgb(100, 180, 255), max_w, self.height,
                    );
                }
                SEP_ROW | COL_SEP_ROW => {
                    let text = "─".repeat(60);
                    Self::draw_text_row(
                        &mut engine, canvas, stride, &rect, PADDING, y,
                        &text, 8.0, color_rgb(60, 60, 60), max_w, self.height,
                    );
                }
                COL_HEADER_ROW => {
                    let ind = state.sort_order.indicator();
                    let ni = if state.sort_column == SortColumn::Name { ind } else { " " };
                    let si = if state.sort_column == SortColumn::Size { ind } else { " " };
                    let mi = if state.sort_column == SortColumn::Modified { ind } else { " " };
                    let text = format!(
                        "    Name {}{:<12} {:>8}{}  {}{}",
                        ni, "", "Size", si, "Modified", mi
                    );
                    Self::draw_text_row(
                        &mut engine, canvas, stride, &rect, PADDING, y,
                        &text, 11.0, color_rgb(120, 120, 120), max_w, self.height,
                    );
                }
                PARENT_ROW => {
                    Self::draw_text_row(
                        &mut engine, canvas, stride, &rect, PADDING, y,
                        "📁  ../", 11.0, color_rgb(150, 150, 220), max_w, self.height,
                    );
                }
                _ => {
                    let entry_idx = virt_idx - FIXED_ROWS;
                    if entry_idx >= state.entries.len() {
                        continue;
                    }
                    let entry = &state.entries[entry_idx];
                    let selected = state.selected_index == Some(entry_idx);
                    let color = if selected {
                        color_rgb(80, 200, 255)
                    } else if entry.is_dir {
                        color_rgb(220, 180, 80)
                    } else {
                        color_rgb(190, 190, 190)
                    };
                    let line = if selected {
                        format!("▶ {}", entry.display_line().trim_start())
                    } else {
                        entry.display_line()
                    };
                    Self::draw_text_row(
                        &mut engine, canvas, stride, &rect, PADDING, y,
                        &line, 11.0, color, max_w, self.height,
                    );
                }
            }
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::Scroll { dy, .. } => {
                self.viewport.scroll(*dy, 1.0 / 60.0);
                EventResponse::Handled
            }

            WidgetEvent::PointerPress { x, y, button: 0x110 } => {
                match self.y_to_hit(*y) {
                    Some(YHit::ColumnHeader) => {
                        let col = self.x_to_sort_column(*x);
                        self.state.borrow_mut().set_sort(col);
                        self.refresh_viewport();
                        EventResponse::Handled
                    }
                    Some(YHit::Parent) => {
                        self.state.borrow_mut().go_parent();
                        self.refresh_viewport();
                        EventResponse::Handled
                    }
                    Some(YHit::Entry(idx)) => {
                        let mut state = self.state.borrow_mut();
                        if idx < state.entries.len() && state.entries[idx].is_dir {
                            let path = state.current_path.join(&state.entries[idx].name);
                            state.navigate(path);
                            drop(state);
                            self.refresh_viewport();
                        } else if idx < state.entries.len() {
                            state.selected_index = Some(idx);
                        }
                        EventResponse::Handled
                    }
                    None => EventResponse::Ignored,
                }
            }

            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                if ke.modifiers.ctrl && ke.keysym == Keysym::h {
                    self.state.borrow_mut().toggle_hidden();
                    self.refresh_viewport();
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::BackSpace {
                    self.state.borrow_mut().go_parent();
                    self.refresh_viewport();
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
                        self.ensure_selected_visible();
                    }
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::Return {
                    let mut state = self.state.borrow_mut();
                    if let Some(idx) = state.selected_index {
                        if idx < state.entries.len() && state.entries[idx].is_dir {
                            let path = state.current_path.join(&state.entries[idx].name);
                            state.navigate(path);
                            drop(state);
                            self.refresh_viewport();
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
                        self.state.borrow_mut().set_sort(c);
                        self.refresh_viewport();
                        return EventResponse::Handled;
                    }
                }
                if ke.modifiers.ctrl && ke.keysym == Keysym::v {
                    eprintln!(
                        "[file_ops] Ctrl+V: paste into {} (stub)",
                        self.state.borrow().current_path.display()
                    );
                    return EventResponse::Handled;
                }
                if ke.keysym == Keysym::Delete {
                    eprintln!(
                        "[file_ops] Delete: request delete in {} (stub)",
                        self.state.borrow().current_path.display()
                    );
                    return EventResponse::Handled;
                }
                let delta = match ke.keysym {
                    Keysym::Page_Up => Some(-self.height * 0.8),
                    Keysym::Page_Down => Some(self.height * 0.8),
                    Keysym::Home => Some(-self.viewport.scroll_offset()),
                    Keysym::End => Some(self.viewport.content_height()),
                    _ => None,
                };
                if let Some(d) = delta {
                    self.viewport.scroll(d, 1.0 / 60.0);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }

            _ => EventResponse::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        true
    }
}
