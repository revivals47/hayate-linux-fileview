//! Tab bar: multiple directory tabs above the breadcrumb bar.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_platform::render::{FontFamily, Renderer, TextEngine, TextParams, VariableFontAxes};
use hayate_platform::scroll::delegate::ItemRect;
use tiny_skia::Color;

use crate::entry::{SortColumn, SortOrder};
use crate::state::ViewMode;

pub(crate) const TAB_HEIGHT: f32 = 22.0;
const TAB_MIN_W: f32 = 80.0;
const TAB_MAX_W: f32 = 200.0;
const TAB_PAD: f32 = 8.0;
const CLOSE_W: f32 = 16.0;

const BG: [u8; 4] = [32, 26, 24, 255];       // bg_primary BGRA
const ACTIVE_BG: [u8; 4] = [48, 42, 40, 255]; // bg_tertiary BGRA
fn text_color() -> Color { Color::from_rgba8(170, 174, 182, 255) } // fg_secondary
fn active_text() -> Color { Color::from_rgba8(56, 164, 240, 255) } // accent
const ACCENT: [u8; 4] = [240, 164, 56, 255];  // accent BGRA

/// Snapshot of per-tab state for fast switch.
#[derive(Clone)]
pub(crate) struct TabSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) cursor: Option<usize>,
    pub(crate) selected: HashSet<usize>,
    pub(crate) show_hidden: bool,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_order: SortOrder,
    pub(crate) view_mode: ViewMode,
    pub(crate) scroll_offset: f32,
}

pub(crate) struct TabInfo {
    pub(crate) label: String,
    pub(crate) snapshot: TabSnapshot,
}

/// Action returned by TabBar::event() for the parent to execute.
pub(crate) enum TabAction {
    None,
    Switch(usize),
    Close(usize),
    NewTab,
}

pub(crate) struct TabBar {
    pub(crate) tabs: Vec<TabInfo>,
    pub(crate) active: usize,
    engine: Rc<RefCell<TextEngine>>,
    width: f32,
}

impl TabBar {
    pub(crate) fn new(initial_path: PathBuf, engine: Rc<RefCell<TextEngine>>) -> Self {
        let label = dir_label(&initial_path);
        let tab = TabInfo {
            label,
            snapshot: TabSnapshot {
                path: initial_path,

                cursor: None,
                selected: HashSet::new(),
                show_hidden: false,
                sort_column: SortColumn::Name,
                sort_order: SortOrder::Asc,
                view_mode: ViewMode::Detail,
                scroll_offset: 0.0,
            },
        };
        Self { tabs: vec![tab], active: 0, engine, width: 0.0 }
    }

    pub(crate) fn tab_count(&self) -> usize { self.tabs.len() }

    pub(crate) fn add_tab(&mut self, path: PathBuf) {
        let label = dir_label(&path);
        self.tabs.push(TabInfo {
            label,
            snapshot: TabSnapshot {
                path,

                cursor: None,
                selected: HashSet::new(),
                show_hidden: false,
                sort_column: SortColumn::Name,
                sort_order: SortOrder::Asc,
                view_mode: ViewMode::Detail,
                scroll_offset: 0.0,
            },
        });
        self.active = self.tabs.len() - 1;
    }

    pub(crate) fn close_tab(&mut self, idx: usize) -> bool {
        if self.tabs.len() <= 1 { return false; }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > idx {
            self.active -= 1;
        }
        true
    }

    pub(crate) fn update_active_label(&mut self, path: &std::path::Path) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.label = dir_label(path);
            tab.snapshot.path = path.to_path_buf();
        }
    }

    /// Layout: receive total width.
    pub(crate) fn layout(&mut self, width: f32) { self.width = width; }

    fn tab_width(&self) -> f32 {
        let n = self.tabs.len().max(1) as f32;
        (self.width / n).clamp(TAB_MIN_W, TAB_MAX_W)
    }

    /// Paint the tab bar.
    pub(crate) fn paint(&mut self, renderer: &mut Renderer, rect: ItemRect) {
        if let Some((canvas, stride)) = renderer.pixels_mut() {
            // Background
            fill_rect_bgra(canvas, &rect, stride, BG);
            let tw = self.tab_width();
            let mut engine = self.engine.borrow_mut();
            for (i, tab) in self.tabs.iter().enumerate() {
                let tx = rect.x + i as f32 * tw;
                let tab_rect = ItemRect::new(tx, rect.y, tw, TAB_HEIGHT);
                if i == self.active {
                    fill_rect_bgra(canvas, &tab_rect, stride, ACTIVE_BG);
                    // Accent underline
                    let ul = ItemRect::new(tx, rect.y + TAB_HEIGHT - 2.0, tw, 2.0);
                    fill_rect_bgra(canvas, &ul, stride, ACCENT);
                }
                // Label
                let color = if i == self.active { active_text() } else { text_color() };
                let max_label_w = tw - TAB_PAD * 2.0 - CLOSE_W;
                let buf = engine.layout(&TextParams {
                    text: &tab.label, font_size: 11.0, line_height: TAB_HEIGHT,
                    color, family: FontFamily::SansSerif, axes: VariableFontAxes::default(),
                }, max_label_w);
                let lx = (tx + TAB_PAD) as i32;
                let ly = rect.y as i32 + 2;
                let buf_w = stride / 4;
                let buf_h = canvas.len() as u32 / stride;
                engine.draw_buffer(&buf, canvas, stride, lx, ly, color, buf_w, buf_h);
                // Close button "×" (only if >1 tab)
                if self.tabs.len() > 1 {
                    let cx = tx + tw - CLOSE_W - 2.0;
                    let cbuf = engine.layout(&TextParams {
                        text: "×", font_size: 12.0, line_height: TAB_HEIGHT,
                        color: text_color(), family: FontFamily::SansSerif, axes: VariableFontAxes::default(),
                    }, CLOSE_W);
                    engine.draw_buffer(&cbuf, canvas, stride, cx as i32, ly, text_color(), buf_w, buf_h);
                }
            }
        }
    }

    /// Handle pointer events; returns action for parent to execute.
    pub(crate) fn handle_click(&self, x: f32) -> TabAction {
        let tw = self.tab_width();
        let idx = (x / tw) as usize;
        if idx >= self.tabs.len() { return TabAction::None; }
        // Check if close button was clicked
        let tab_x = idx as f32 * tw;
        if self.tabs.len() > 1 && x > tab_x + tw - CLOSE_W - 4.0 {
            return TabAction::Close(idx);
        }
        if idx != self.active {
            TabAction::Switch(idx)
        } else {
            TabAction::None
        }
    }

    /// Handle keyboard shortcuts; returns action.
    pub(crate) fn handle_key(&self, ke: &hayate_platform::platform::keyboard::KeyEvent) -> TabAction {
        use hayate_platform::platform::keyboard::KeyState;
        use xkbcommon::xkb::Keysym;
        if ke.state != KeyState::Pressed { return TabAction::None; }
        if ke.modifiers.ctrl {
            match ke.keysym {
                Keysym::t | Keysym::T => return TabAction::NewTab,
                Keysym::w | Keysym::W => {
                    if self.tabs.len() > 1 { return TabAction::Close(self.active); }
                }
                Keysym::Tab => {
                    let next = if ke.modifiers.shift {
                        (self.active + self.tabs.len() - 1) % self.tabs.len()
                    } else {
                        (self.active + 1) % self.tabs.len()
                    };
                    if next != self.active { return TabAction::Switch(next); }
                }
                k @ (Keysym::_1 | Keysym::_2 | Keysym::_3 | Keysym::_4 |
                     Keysym::_5 | Keysym::_6 | Keysym::_7 | Keysym::_8 | Keysym::_9) => {
                    let n = (k.raw() - Keysym::_1.raw()) as usize;
                    if n < self.tabs.len() { return TabAction::Switch(n); }
                }
                _ => {}
            }
        }
        TabAction::None
    }
}

/// Save the current FileViewState into a TabSnapshot.
pub(crate) fn save_snapshot(
    state: &Rc<RefCell<crate::state::FileViewState>>,
    file_list: &crate::file_list::FileListWidget,
) -> TabSnapshot {
    let st = state.borrow();
    TabSnapshot {
        path: st.current_path.clone(),
        cursor: st.cursor,
        selected: st.selected.clone(),
        show_hidden: st.show_hidden,
        sort_column: st.sort_column,
        sort_order: st.sort_order,
        view_mode: st.view_mode,
        scroll_offset: file_list.viewport_offset(),
    }
}

/// Restore a TabSnapshot into the FileViewState.
pub(crate) fn restore_snapshot(
    snap: &TabSnapshot,
    state: &Rc<RefCell<crate::state::FileViewState>>,
    file_list: &mut crate::file_list::FileListWidget,
) {
    let mut st = state.borrow_mut();
    if st.current_path != snap.path {
        st.current_path = snap.path.clone();
        st.show_hidden = snap.show_hidden;
        st.sort_column = snap.sort_column;
        st.sort_order = snap.sort_order;
        st.view_mode = snap.view_mode;
        st.refresh();
        let p = st.current_path.clone();
        if let Some(ref mut w) = st.fs_watcher { w.watch(&p); }
    }
    st.cursor = snap.cursor;
    st.selected = snap.selected.clone();
    drop(st);
    file_list.set_viewport_offset(snap.scroll_offset);
}

/// Draw a 1px accent line (used for focus indicator).
pub(crate) fn paint_hline(renderer: &mut Renderer, x: f32, y: f32, w: f32) {
    if let Some((canvas, stride)) = renderer.pixels_mut() {
        fill_rect_bgra(canvas, &ItemRect::new(x, y, w, 1.0), stride, ACCENT);
    }
}

fn dir_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string())
}

fn fill_rect_bgra(canvas: &mut [u8], rect: &ItemRect, stride: u32, bgra: [u8; 4]) {
    let x0 = rect.x.max(0.0) as u32;
    let y0 = rect.y.max(0.0) as u32;
    let x1 = rect.right() as u32;
    let y1 = rect.bottom() as u32;
    for py in y0..y1 {
        let row = (py * stride) as usize;
        for px in x0..x1 {
            let o = row + (px * 4) as usize;
            if o + 3 < canvas.len() {
                canvas[o] = bgra[0]; canvas[o+1] = bgra[1];
                canvas[o+2] = bgra[2]; canvas[o+3] = bgra[3];
            }
        }
    }
}
