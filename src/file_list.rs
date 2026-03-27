//! Virtualized file list widget using VirtualViewport + direct TextEngine rendering.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use tiny_skia::Color;

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
pub(crate) const JUMP_TIMEOUT_MS: u128 = 300;

fn color_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

// ── FileListWidget ──

pub(crate) struct FileListWidget {
    pub(crate) state: Rc<RefCell<FileViewState>>,
    pub(crate) viewport: VirtualViewport,
    engine: Rc<RefCell<TextEngine>>,
    width: f32,
    pub(crate) height: f32,
    ctrl_held: bool,
    shift_held: bool,
    pub(crate) clipboard: Vec<PathBuf>,
    pub(crate) jump_buffer: String,
    pub(crate) jump_last_input: Option<Instant>,
    pub(crate) search_mode: bool,
    pub(crate) last_file_op: Option<Instant>,
    is_dirty: bool,
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
            clipboard: Vec::new(),
            width: 0.0,
            height: 0.0,
            ctrl_held: false,
            shift_held: false,
            jump_buffer: String::new(),
            jump_last_input: None,
            search_mode: false,
            last_file_op: None,
            is_dirty: true,
        }
    }

    pub(crate) fn state(&self) -> &Rc<RefCell<FileViewState>> {
        &self.state
    }

    /// Refresh after external state changes (e.g. sidebar navigation).
    pub(crate) fn rebuild(&mut self) {
        self.refresh_viewport();
    }

    pub(crate) fn refresh_viewport(&mut self) {
        let state = self.state.borrow();
        let count = match &state.filtered_indices {
            Some(fi) => fi.len(),
            None => state.entries.len(),
        };
        drop(state);
        self.viewport.on_total_changed(FIXED_ROWS + count);
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
        if let Some(idx) = self.state.borrow().cursor {
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
                let vis_idx = r - FIXED_ROWS;
                let state = self.state.borrow();
                let entry_idx = match &state.filtered_indices {
                    Some(fi) => fi.get(vis_idx).copied(),
                    None if vis_idx < state.entries.len() => Some(vis_idx),
                    _ => None,
                };
                entry_idx.map(YHit::Entry)
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

    pub(crate) fn jump_to_prefix(&mut self) {
        let st = self.state.borrow();
        let prefix = self.jump_buffer.to_lowercase();
        let count = st.entries.len();
        if count == 0 || prefix.is_empty() { return; }
        let start = st.cursor.map(|c| c + 1).unwrap_or(0);
        let found = (0..count).map(|i| (start + i) % count)
            .find(|&i| st.entries[i].name.to_lowercase().starts_with(&prefix));
        drop(st);
        if let Some(idx) = found {
            self.state.borrow_mut().select_single(idx);
            self.ensure_cursor_visible();
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
                    let search_indicator = match &state.search_query {
                        Some(q) => format!("  [Search: {}]", q),
                        None => String::new(),
                    };
                    let text = format!("  {}{}{}", state.current_path.display(), hidden, search_indicator);
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
                    let vis_idx = virt_idx - FIXED_ROWS;
                    let entry_idx = match &state.filtered_indices {
                        Some(fi) => {
                            if vis_idx >= fi.len() { continue; }
                            fi[vis_idx]
                        }
                        None => vis_idx,
                    };
                    if entry_idx >= state.entries.len() {
                        continue;
                    }
                    let entry = &state.entries[entry_idx];
                    let selected = state.is_selected(entry_idx);
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
        let result = self.handle_event_inner(event);
        if result == EventResponse::Handled {
            self.is_dirty = true;
        }
        result
    }
}

// Event handling split out so dirty flag is set automatically.
impl FileListWidget {
    fn handle_event_inner(&mut self, event: &WidgetEvent) -> EventResponse {
        // Track modifier keys from key events (for non-pointer use)
        if let WidgetEvent::Key(ke) = event {
            self.ctrl_held = ke.modifiers.ctrl;
            self.shift_held = ke.modifiers.shift;
        }
        match event {
            WidgetEvent::Scroll { dy, .. } => {
                self.viewport.scroll(*dy, 1.0 / 60.0);
                EventResponse::Handled
            }

            WidgetEvent::PointerPress { x, y, button: 0x110, modifiers } => {
                // Use modifiers from the pointer event directly
                self.ctrl_held = modifiers.ctrl;
                self.shift_held = modifiers.shift;
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
                        if idx < state.entries.len() && state.entries[idx].is_dir && !self.ctrl_held && !self.shift_held {
                            let path = state.current_path.join(&state.entries[idx].name);
                            state.navigate(path);
                            drop(state);
                            self.refresh_viewport();
                        } else if idx < state.entries.len() {
                            if self.ctrl_held {
                                state.toggle_select(idx);
                            } else if self.shift_held {
                                let anchor = state.anchor.unwrap_or(0);
                                state.select_range(anchor, idx);
                            } else {
                                state.select_single(idx);
                            }
                        }
                        EventResponse::Handled
                    }
                    None => EventResponse::Ignored,
                }
            }

            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                crate::keybindings::handle_key_event(self, ke)
            }

            _ => EventResponse::Ignored,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        self.is_dirty || self.viewport.is_animating()
    }

    fn clear_dirty(&mut self) {
        self.is_dirty = false;
    }
}
