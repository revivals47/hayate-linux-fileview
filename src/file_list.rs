//! Virtualized file list widget using VirtualViewport + direct TextEngine rendering.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use tiny_skia::Color;

use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::render::{FontFamily, Renderer, TextEngine, TextParams};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::scroll::physics::PixelScrollPhysics;
use hayate_ui::scroll::viewport::VirtualViewport;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::entry::SortColumn;
use crate::state::{FileViewState, ViewMode};

// ── Layout constants ──

const ROW_HEIGHT: f32 = 18.0;
const COMPACT_COLS: usize = 3;
/// Fixed rows: header, separator, column header, column separator, parent (..)
const FIXED_ROWS: usize = 5;
const HEADER_ROW: usize = 0;
const SEP_ROW: usize = 1;
const COL_HEADER_ROW: usize = 2;
const COL_SEP_ROW: usize = 3;
const PARENT_ROW: usize = 4;
const PADDING: f32 = 12.0;
pub(crate) const JUMP_TIMEOUT_MS: u128 = 300;
const TEXT_CACHE_CAP: usize = 200;

fn color_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

// ── FileListWidget ──

pub(crate) struct FileListWidget {
    pub(crate) state: Rc<RefCell<FileViewState>>,
    pub(crate) viewport: VirtualViewport,
    engine: Rc<RefCell<TextEngine>>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    ctrl_held: bool,
    shift_held: bool,
    pub(crate) clipboard: Vec<PathBuf>,
    pub(crate) jump_buffer: String,
    pub(crate) jump_last_input: Option<Instant>,
    pub(crate) search_mode: bool,
    pub(crate) last_file_op: Option<Instant>,
    pub(crate) rename_state: Option<crate::rename_ui::RenameState>,
    pub(crate) context_menu: hayate_ui::widget::ContextMenu,
    pub(crate) pending_file_paste: bool,
    pub(crate) toast: Rc<RefCell<hayate_ui::widget::toast::ToastWidget>>,
    is_dirty: bool,
    /// LRU cache of cosmic_text Buffers keyed by (row_text, font_size_bits).
    pub(crate) text_cache: RefCell<crate::lru_cache::LruCache<(String, u32), cosmic_text::Buffer>>,
}

impl FileListWidget {
    pub(crate) fn new(
        state: Rc<RefCell<FileViewState>>,
        toast: Rc<RefCell<hayate_ui::widget::toast::ToastWidget>>,
    ) -> Self {
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
            rename_state: None,
            context_menu: crate::context_handler::build_menu(),
            pending_file_paste: false,
            toast,
            is_dirty: true,
            text_cache: RefCell::new(crate::lru_cache::LruCache::new(TEXT_CACHE_CAP)),
        }
    }

    pub(crate) fn state(&self) -> &Rc<RefCell<FileViewState>> {
        &self.state
    }

    pub(crate) fn rebuild(&mut self) { self.refresh_viewport(); }
    pub(crate) fn viewport_offset(&self) -> f32 { self.viewport.scroll_offset() }
    pub(crate) fn set_viewport_offset(&mut self, offset: f32) {
        let delta = offset - self.viewport.scroll_offset();
        if delta.abs() > 0.1 { self.viewport.scroll(delta, 1.0); }
    }

    pub(crate) fn refresh_viewport(&mut self) {
        let state = self.state.borrow();
        let entry_count = match &state.filtered_indices {
            Some(fi) => fi.len(),
            None => state.entries.len(),
        };
        let virtual_rows = match state.view_mode {
            ViewMode::Compact => (entry_count + COMPACT_COLS - 1) / COMPACT_COLS.max(1),
            _ => entry_count,
        };
        drop(state);
        self.viewport.on_total_changed(FIXED_ROWS + virtual_rows);
        self.text_cache.borrow_mut().clear();
    }

    pub(crate) fn ensure_cursor_visible(&mut self) {
        if let Some(idx) = self.state.borrow().cursor {
            self.viewport.scroll_to_item(FIXED_ROWS + idx);
        }
    }

    pub(crate) fn y_x_to_hit(&self, y: f32, x: f32) -> Option<YHit> {
        let content_y = y + self.viewport.scroll_offset();
        if content_y < 0.0 { return None; }
        let row = (content_y / ROW_HEIGHT) as usize;
        match row {
            COL_HEADER_ROW => Some(YHit::ColumnHeader),
            PARENT_ROW => Some(YHit::Parent),
            r if r >= FIXED_ROWS => {
                let virt_row = r - FIXED_ROWS;
                let state = self.state.borrow();
                let vis_idx = if state.view_mode == ViewMode::Compact {
                    let col = ((x - PADDING).max(0.0) / (self.width / COMPACT_COLS as f32)) as usize;
                    virt_row * COMPACT_COLS + col.min(COMPACT_COLS - 1)
                } else {
                    virt_row
                };
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

    #[allow(clippy::too_many_arguments)]
    fn draw_text_row(
        cache: &RefCell<crate::lru_cache::LruCache<(String, u32), cosmic_text::Buffer>>,
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
        let key = (text.to_string(), font_size.to_bits());
        let mut text_cache = cache.borrow_mut();
        let buffer = text_cache.get_or_insert_with(key, || {
            let params = TextParams {
                text,
                font_size,
                line_height: ROW_HEIGHT,
                color,
                family: FontFamily::Monospace,
            };
            engine.layout(&params, max_w)
        });
        engine.draw_buffer(
            buffer,
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

pub(crate) enum YHit {
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

    fn paint(&mut self, renderer: &mut Renderer, rect: ItemRect) {
        if let Some((canvas, stride)) = renderer.pixels_mut() {
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
                        Self::draw_text_row(&self.text_cache,
                            &mut engine, canvas, stride, &rect, PADDING, y,
                            &text, 16.0, color_rgb(100, 180, 255), max_w, self.height,
                        );
                    }
                    SEP_ROW | COL_SEP_ROW => {
                        let text = "─".repeat(60);
                        Self::draw_text_row(&self.text_cache,
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
                        Self::draw_text_row(&self.text_cache,
                            &mut engine, canvas, stride, &rect, PADDING, y,
                            &text, 11.0, color_rgb(120, 120, 120), max_w, self.height,
                        );
                    }
                    PARENT_ROW => {
                        Self::draw_text_row(&self.text_cache,
                            &mut engine, canvas, stride, &rect, PADDING, y,
                            "📁  ../", 11.0, color_rgb(150, 150, 220), max_w, self.height,
                        );
                    }
                    _ => {
                        let virt_row = virt_idx - FIXED_ROWS;
                        match state.view_mode {
                            ViewMode::Detail => {
                                let entry_idx = resolve_vis_idx(&state, virt_row);
                                let Some(entry_idx) = entry_idx else { continue };
                                let entry = &state.entries[entry_idx];
                                let selected = state.is_selected(entry_idx);
                                let color = entry_color(selected, entry.is_dir);
                                let line = if selected {
                                    format!("▶ {}", entry.display_line().trim_start())
                                } else {
                                    entry.display_line()
                                };
                                Self::draw_text_row(&self.text_cache,
                                    &mut engine, canvas, stride, &rect, PADDING, y,
                                    &line, 11.0, color, max_w, self.height,
                                );
                            }
                            ViewMode::List => {
                                let entry_idx = resolve_vis_idx(&state, virt_row);
                                let Some(entry_idx) = entry_idx else { continue };
                                let entry = &state.entries[entry_idx];
                                let selected = state.is_selected(entry_idx);
                                let color = entry_color(selected, entry.is_dir);
                                let icon = if entry.is_dir { "📁 " } else { "   " };
                                let name: String = entry.name.chars().take(40).collect();
                                let line = if selected {
                                    format!("▶ {}{}", icon.trim_start(), name)
                                } else {
                                    format!("{}{}", icon, name)
                                };
                                Self::draw_text_row(&self.text_cache,
                                    &mut engine, canvas, stride, &rect, PADDING, y,
                                    &line, 11.0, color, max_w, self.height,
                                );
                            }
                            ViewMode::Compact => {
                                let col_w = (self.width - PADDING * 2.0) / COMPACT_COLS as f32;
                                for col in 0..COMPACT_COLS {
                                    let vis_idx = virt_row * COMPACT_COLS + col;
                                    let entry_idx = resolve_vis_idx(&state, vis_idx);
                                    let Some(entry_idx) = entry_idx else { continue };
                                    let entry = &state.entries[entry_idx];
                                    let selected = state.is_selected(entry_idx);
                                    let color = entry_color(selected, entry.is_dir);
                                    let icon = if entry.is_dir { "📁 " } else { "   " };
                                    let name: String = entry.name.chars().take(14).collect();
                                    let line = if selected {
                                        format!("▶{}{}", icon.trim_start(), name)
                                    } else {
                                        format!("{}{}", icon, name)
                                    };
                                    let cx = PADDING + col as f32 * col_w;
                                    Self::draw_text_row(&self.text_cache,
                                        &mut engine, canvas, stride, &rect, cx, y,
                                        &line, 10.0, color, col_w, self.height,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        // Overlay: rename text input on top of the target entry row
        if let Some(ref mut rs) = self.rename_state {
            let max_w = (self.width - PADDING * 2.0).max(0.0);
            let virt = FIXED_ROWS + rs.entry_idx;
            let ry = self.viewport.item_y_in_viewport(virt);
            rs.paint(renderer, ItemRect::new(rect.x + PADDING, rect.y + ry, max_w, ROW_HEIGHT));
        }
        // Context menu overlay
        crate::context_handler::paint_menu(&mut self.context_menu, &self.engine, &self.text_cache, renderer, rect);
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
        // Context menu intercept — consumes all events while visible
        if self.context_menu.is_visible() {
            self.context_menu.event(event);
            if let Some(id) = self.context_menu.take_selected() { crate::context_handler::dispatch(self, &id); }
            return EventResponse::Handled;
        }
        // Rename mode: delegate events to rename UI
        if self.rename_state.is_some() {
            return crate::rename_ui::handle_rename_event(self, event);
        }
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
                match self.y_x_to_hit(*y, *x) {
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
                        if idx >= state.entries.len() {
                            return EventResponse::Ignored;
                        }
                        if self.ctrl_held {
                            state.toggle_select(idx);
                        } else if self.shift_held {
                            let anchor = state.anchor.unwrap_or(0);
                            state.select_range(anchor, idx);
                        } else {
                            state.select_single(idx);
                        }
                        EventResponse::Handled
                    }
                    None => EventResponse::Ignored,
                }
            }

            WidgetEvent::PointerPress { x, y, button: 0x111, .. } => {
                crate::context_handler::show_at(self, *x, *y);
                EventResponse::Handled
            }

            WidgetEvent::FileDrop { uris, .. } => {
                crate::keybindings::handle_clipboard_paste(self, &uris.join("\n"));
                EventResponse::Handled
            }

            WidgetEvent::DoubleClick { x, y, button: 0x110, .. } => {
                match self.y_x_to_hit(*y, *x) {
                    Some(YHit::Entry(idx)) => {
                        let state = self.state.borrow();
                        if idx >= state.entries.len() {
                            return EventResponse::Ignored;
                        }
                        let path = state.current_path.join(&state.entries[idx].name);
                        if state.entries[idx].is_dir {
                            drop(state);
                            self.state.borrow_mut().navigate(path);
                            self.refresh_viewport();
                        } else {
                            drop(state);
                            open_with_xdg(&path);
                        }
                        EventResponse::Handled
                    }
                    Some(YHit::Parent) => {
                        self.state.borrow_mut().go_parent();
                        self.refresh_viewport();
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            }

            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                crate::keybindings::handle_key_event(self, ke)
            }

            // System clipboard paste result (file URIs from Ctrl+V)
            WidgetEvent::ImeCommit(text) if self.pending_file_paste => {
                self.pending_file_paste = false;
                crate::keybindings::handle_clipboard_paste(self, text);
                EventResponse::Handled
            }

            _ => EventResponse::Ignored,
        }
    }

    pub(crate) fn dirty(&self) -> bool { self.is_dirty || self.viewport.is_animating() }
    pub(crate) fn clear_dirty(&mut self) { self.is_dirty = false; }
}

fn resolve_vis_idx(state: &FileViewState, vis_idx: usize) -> Option<usize> {
    match &state.filtered_indices {
        Some(fi) => fi.get(vis_idx).copied(),
        None if vis_idx < state.entries.len() => Some(vis_idx),
        _ => None,
    }
}

fn entry_color(sel: bool, is_dir: bool) -> Color {
    if sel { color_rgb(56, 164, 240) } else if is_dir { color_rgb(226, 200, 100) } else { color_rgb(170, 174, 182) }
}

pub(crate) fn open_with_xdg(path: &std::path::Path) {
    use std::process::{Command, Stdio};
    let _ = Command::new("xdg-open").arg(path)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
