//! Three-pane layout: sidebar | file list | preview, with a status bar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use hayate_ui::render::{Renderer, TextEngine};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_ui::widget::toast::ToastWidget;
use hayate_ui::widget::{alloc_widget_id, WidgetId};

use crate::breadcrumb::BreadcrumbWidget;
use crate::file_list::FileListWidget;
use crate::preview::PreviewPane;
use crate::sidebar::SidebarWidget;
use crate::state::FileViewState;
use crate::status_bar::{StatusBar, StatusInfo};
use crate::terminal_widget::TerminalWidget;

const BREADCRUMB_HEIGHT: f32 = 24.0;
const STATUS_HEIGHT: f32 = 20.0;
/// Minimum pane widths (pixels).
const MIN_SIDEBAR: f32 = 80.0;
const MIN_LIST: f32 = 150.0;
const MIN_PREVIEW: f32 = 100.0;
/// Hit zone half-width for divider detection (pixels).
const DIVIDER_HIT: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum PaneFocus { Sidebar, FileList, Preview }

#[derive(Clone, Copy)]
enum DragTarget { SidebarRight, PreviewLeft }

struct DragState {
    target: DragTarget,
    start_x: f32,
    original_width: f32,
}

pub(crate) struct ThreePaneWidget {
    /// Stable widget identity (Phase 5: `Widget::id` is required).
    id: WidgetId,
    breadcrumb: BreadcrumbWidget,
    sidebar: SidebarWidget,
    file_list: FileListWidget,
    preview: PreviewPane,
    status_bar: StatusBar,
    sidebar_width: f32,
    list_width: f32,
    preview_width: f32,
    total_width: f32,
    last_pointer_x: f32,
    dragging: Option<DragState>,
    /// User-fixed sidebar ratio (None = auto-responsive).
    pub(crate) sidebar_ratio: Option<f32>,
    /// User-fixed preview ratio (None = auto-responsive).
    pub(crate) preview_ratio: Option<f32>,
    /// Shared cursor shape buffer — set to change the mouse cursor.
    cursor_shape: Option<Rc<Cell<Option<hayate_ui::platform::CursorShape>>>>,
    toast: Rc<RefCell<ToastWidget>>,
    focus: PaneFocus,
    tab_bar: crate::tab_bar::TabBar,
    state: Rc<RefCell<FileViewState>>,
    terminal: TerminalWidget,
    terminal_visible: bool,
}

impl ThreePaneWidget {
    pub(crate) fn new(
        state: Rc<RefCell<FileViewState>>,
        engine: Rc<RefCell<TextEngine>>,
    ) -> Self {
        let toast = Rc::new(RefCell::new(ToastWidget::new(engine.clone())));
        let breadcrumb = BreadcrumbWidget::new(Rc::clone(&state), engine.clone());
        let sidebar = SidebarWidget::new(Rc::clone(&state), engine.clone());
        let file_list = FileListWidget::new(Rc::clone(&state), Rc::clone(&toast));
        let preview = PreviewPane::new(Rc::clone(&state), engine.clone());
        let mut status_bar = StatusBar::new(engine.clone());
        // Initial status
        {
            let st = state.borrow();
            status_bar.update(&StatusInfo {
                item_count: st.entries.len(),
                show_hidden: st.show_hidden,
                selected_name: None,
                selected_size: None,
                selected_count: 0,
                selected_total_size: 0,
                current_path: &st.current_path,
                error: None,
            });
        }
        let sidebar_ratio = state.borrow().sidebar_ratio.get();
        let preview_ratio = state.borrow().preview_ratio.get();
        let initial_path = state.borrow().current_path.clone();
        let tab_bar = crate::tab_bar::TabBar::new(initial_path, engine.clone());
        let terminal = TerminalWidget::new(engine);
        Self {
            id: alloc_widget_id(),
            breadcrumb, sidebar, file_list, preview, status_bar,
            sidebar_width: 0.0, list_width: 0.0, preview_width: 0.0,
            total_width: 0.0, last_pointer_x: 0.0, dragging: None,
            sidebar_ratio, preview_ratio, cursor_shape: None,
            toast, focus: PaneFocus::FileList,
            tab_bar, state: Rc::clone(&state),
            terminal, terminal_visible: false,
        }
    }

    /// Inject the cursor shape buffer from App (called after construction).
    pub(crate) fn set_cursor_shape_buffer(&mut self, buf: Rc<Cell<Option<hayate_ui::platform::CursorShape>>>) {
        self.cursor_shape = Some(buf);
    }

    fn set_cursor(&self, shape: hayate_ui::platform::CursorShape) {
        if let Some(ref buf) = self.cursor_shape {
            buf.set(Some(shape));
        }
    }

    fn update_status(&mut self) {
        let st = self.file_list.state().borrow();
        let sc = st.selected.len();
        let sts: u64 = st.selected.iter().filter_map(|&i| st.entries.get(i)).map(|e| e.size).sum();
        let (sn, ss) = match st.cursor {
            Some(i) if i < st.entries.len() => {
                let e = &st.entries[i];
                (Some(e.name.as_str()), if e.is_dir { None } else { Some(e.format_size()) })
            }
            _ => (None, None),
        };
        self.status_bar.update(&StatusInfo {
            item_count: st.entries.len(), show_hidden: st.show_hidden,
            selected_name: sn, selected_size: ss, selected_count: sc,
            selected_total_size: sts, current_path: &st.current_path,
            error: st.last_error.as_deref(),
        });
    }

    fn hit_divider(&self, x: f32, y: f32) -> Option<DragTarget> {
        if y < BREADCRUMB_HEIGHT { return None; }
        let (sw, pw) = (self.sidebar_width, self.preview_width);
        if sw > 0.0 && (x - sw).abs() < DIVIDER_HIT { return Some(DragTarget::SidebarRight); }
        if pw > 0.0 && (x - sw - self.list_width).abs() < DIVIDER_HIT { return Some(DragTarget::PreviewLeft); }
        None
    }

    fn next_focus(&self, reverse: bool) -> PaneFocus {
        next_focus_logic(self.focus, self.sidebar_width > 0.0, self.preview_width > 0.0, reverse)
    }

    /// Sync breadcrumb, preview, and status bar after any navigation.
    fn sync_after_nav(&mut self) {
        self.breadcrumb.update_segments();
        self.file_list.rebuild();
        self.preview.update_preview();
        self.update_status();
        let path = self.state.borrow().current_path.clone();
        self.tab_bar.update_active_label(&path);
        if self.terminal_visible { self.terminal.change_dir(&path); }
    }

    fn save_active_tab(&mut self) {
        self.tab_bar.tabs[self.tab_bar.active].snapshot =
            crate::tab_bar::save_snapshot(&self.state, &self.file_list);
    }
    fn restore_active_tab(&mut self) {
        let snap = self.tab_bar.tabs[self.tab_bar.active].snapshot.clone();
        crate::tab_bar::restore_snapshot(&snap, &self.state, &mut self.file_list);
        self.sync_after_nav();
    }
    fn execute_tab_action(&mut self, action: crate::tab_bar::TabAction) {
        use crate::tab_bar::TabAction;
        match action {
            TabAction::None => {}
            TabAction::NewTab => { self.save_active_tab(); self.tab_bar.add_tab(self.state.borrow().current_path.clone()); }
            TabAction::Switch(i) => { self.save_active_tab(); self.tab_bar.active = i; self.restore_active_tab(); }
            TabAction::Close(i) => { if i == self.tab_bar.active { self.save_active_tab(); } self.tab_bar.close_tab(i); self.restore_active_tab(); }
        }
    }

    /// Apply a drag delta to compute new pane ratios, clamping to min widths.
    fn apply_drag(&mut self, current_x: f32) {
        let Some(ref drag) = self.dragging else { return };
        let (tw, d) = (self.total_width, current_x - drag.start_x);
        if tw <= 0.0 { return; }
        match drag.target {
            DragTarget::SidebarRight => self.sidebar_ratio = Some((drag.original_width + d).clamp(MIN_SIDEBAR, tw - MIN_LIST - self.preview_width) / tw),
            DragTarget::PreviewLeft => self.preview_ratio = Some((drag.original_width - d).clamp(MIN_PREVIEW, tw - self.sidebar_width - MIN_LIST) / tw),
        }
    }
}

impl Widget for ThreePaneWidget {
    fn id(&self) -> WidgetId {
        self.id
    }

    fn layout(&mut self, constraints: &Constraints) -> Size {
        // Poll filesystem watcher for external changes
        {
            let mut state = self.file_list.state().borrow_mut();
            let changed = state.fs_watcher.as_ref().is_some_and(|w| w.needs_refresh());
            if changed {
                state.refresh();
                drop(state);
                self.sync_after_nav();
            }
        }

        let total_width = constraints.max_width;
        self.total_width = total_width;
        let tab_h = if self.tab_bar.tab_count() > 1 { crate::tab_bar::TAB_HEIGHT } else { 0.0 };
        self.tab_bar.layout(total_width);
        let full_content = constraints.max_height - tab_h - BREADCRUMB_HEIGHT - STATUS_HEIGHT;
        let term_h = if self.terminal_visible { (full_content * 0.35).max(80.0).min(full_content - 100.0) } else { 0.0 };
        let content_height = full_content - term_h;
        if self.terminal_visible { self.terminal.poll(); self.terminal.layout(&tight(total_width, term_h)); }

        self.breadcrumb.layout(&tight(total_width, BREADCRUMB_HEIGHT));

        // Pane sizing: user ratios override responsive defaults
        if total_width < 350.0 {
            // Narrow: file list only (ignore user ratios)
            self.sidebar_width = 0.0;
            self.preview_width = 0.0;
            self.list_width = total_width;
        } else if total_width < 550.0 {
            // Medium: sidebar + file list
            self.sidebar_width = match self.sidebar_ratio {
                Some(r) => (r * total_width).clamp(MIN_SIDEBAR, total_width - MIN_LIST),
                None => 120.0_f32.min(total_width * 0.25),
            };
            self.preview_width = 0.0;
            self.list_width = total_width - self.sidebar_width;
        } else {
            // Full: 3 panes
            self.sidebar_width = match self.sidebar_ratio {
                Some(r) => (r * total_width).clamp(MIN_SIDEBAR, total_width - MIN_LIST - MIN_PREVIEW),
                None => 150.0_f32.min(total_width * 0.18),
            };
            let remaining = total_width - self.sidebar_width;
            self.preview_width = match self.preview_ratio {
                Some(r) => (r * total_width).clamp(MIN_PREVIEW, remaining - MIN_LIST),
                None => 250.0_f32.min(remaining * 0.35),
            };
            self.list_width = remaining - self.preview_width;
        }

        if self.sidebar_width > 0.0 { self.sidebar.layout(&tight(self.sidebar_width, content_height)); }
        self.file_list.layout(&tight(self.list_width, content_height));
        if self.preview_width > 0.0 { self.preview.layout(&tight(self.preview_width, content_height)); }
        self.status_bar.layout(&tight(total_width, STATUS_HEIGHT));

        Size::new(total_width, constraints.max_height)
    }

    fn paint(&mut self, renderer: &mut Renderer, rect: ItemRect) {
        let tab_h = if self.tab_bar.tab_count() > 1 { crate::tab_bar::TAB_HEIGHT } else { 0.0 };
        let full_content = rect.height - tab_h - BREADCRUMB_HEIGHT - STATUS_HEIGHT;
        let term_h = if self.terminal_visible { (full_content * 0.35).max(80.0).min(full_content - 100.0) } else { 0.0 };
        let content_height = full_content - term_h;
        let mut cy = rect.y;

        // Tab bar (only if >1 tab)
        if tab_h > 0.0 {
            self.tab_bar.paint(renderer, ItemRect::new(rect.x, cy, rect.width, tab_h));
            cy += tab_h;
        }

        // Breadcrumb bar
        self.breadcrumb.paint(renderer, ItemRect::new(rect.x, cy, rect.width, BREADCRUMB_HEIGHT));
        let pane_y = cy + BREADCRUMB_HEIGHT;

        // Sidebar (skip if collapsed)
        if self.sidebar_width > 0.0 {
            let sidebar_rect =
                ItemRect::new(rect.x, pane_y, self.sidebar_width, content_height);
            self.sidebar.paint(renderer, sidebar_rect);
        }

        // File list (always visible)
        let list_x = rect.x + self.sidebar_width;
        let list_rect = ItemRect::new(list_x, pane_y, self.list_width, content_height);
        self.file_list.paint(renderer, list_rect);

        // Preview (skip if collapsed)
        if self.preview_width > 0.0 {
            let preview_x = list_x + self.list_width;
            let preview_rect =
                ItemRect::new(preview_x, pane_y, self.preview_width, content_height);
            self.preview.paint(renderer, preview_rect);
        }

        // Terminal pane (between file panes and status bar)
        if self.terminal_visible {
            self.terminal.paint(renderer, ItemRect::new(rect.x, pane_y + content_height, rect.width, term_h));
        }

        // Status bar (full width, bottom)
        let status_y = pane_y + content_height + term_h;
        let status_rect = ItemRect::new(rect.x, status_y, rect.width, STATUS_HEIGHT);
        self.status_bar.paint(renderer, status_rect);

        // Focus indicator: thin top border on the focused pane
        let focus_span = match self.focus {
            PaneFocus::Sidebar if self.sidebar_width > 0.0 => Some((rect.x, self.sidebar_width)),
            PaneFocus::FileList => Some((list_x, self.list_width)),
            PaneFocus::Preview if self.preview_width > 0.0 => Some((list_x + self.list_width, self.preview_width)),
            _ => None,
        };
        if let Some((fx, fw)) = focus_span {
            crate::tab_bar::paint_hline(renderer, fx, pane_y, fw);
        }

        // Toast overlay (topmost layer)
        let mut toast = self.toast.borrow_mut();
        toast.tick(0.016);
        if toast.has_visible() {
            toast.paint(renderer, rect);
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        // Track pointer position for scroll routing
        match event {
            WidgetEvent::PointerMove { x, .. }
            | WidgetEvent::PointerPress { x, .. } => {
                self.last_pointer_x = *x;
            }
            _ => {}
        }

        // Divider drag: PointerMove while dragging, or cursor shape on hover
        if let WidgetEvent::PointerMove { x, y, .. } = event {
            if self.dragging.is_some() {
                self.apply_drag(*x);
                return EventResponse::Handled;
            }
            // Update cursor shape based on divider hover
            use hayate_ui::platform::CursorShape;
            if self.hit_divider(*x, *y).is_some() {
                self.set_cursor(CursorShape::ColResize);
            } else {
                self.set_cursor(CursorShape::Default);
            }
        }

        // Divider drag: PointerRelease ends drag → persist ratios to state
        if let WidgetEvent::PointerRelease { .. } = event
            && self.dragging.take().is_some() {
                self.set_cursor(hayate_ui::platform::CursorShape::Default);
                let state = self.file_list.state().borrow();
                state.sidebar_ratio.set(self.sidebar_ratio);
                state.preview_ratio.set(self.preview_ratio);
                return EventResponse::Handled;
            }

        // PointerLeave → restore default cursor
        if matches!(event, WidgetEvent::PointerLeave) {
            self.set_cursor(hayate_ui::platform::CursorShape::Default);
        }

        if let WidgetEvent::PointerPress { x, y, button, modifiers, .. } = event {
            let tab_h = if self.tab_bar.tab_count() > 1 { crate::tab_bar::TAB_HEIGHT } else { 0.0 };
            // Tab bar click
            if tab_h > 0.0 && *y < tab_h {
                let action = self.tab_bar.handle_click(*x);
                self.execute_tab_action(action);
                return EventResponse::Handled;
            }
            let adj_y = *y - tab_h;
            // Divider hit → start drag
            if *button == 0x110
                && let Some(target) = self.hit_divider(*x, adj_y) {
                    let ow = match target { DragTarget::SidebarRight => self.sidebar_width, DragTarget::PreviewLeft => self.preview_width };
                    self.dragging = Some(DragState { target, start_x: *x, original_width: ow });
                    self.set_cursor(hayate_ui::platform::CursorShape::ColResize);
                    return EventResponse::Handled;
                }
            // Breadcrumb bar
            if adj_y < BREADCRUMB_HEIGHT {
                let r = self.breadcrumb.event(event);
                if matches!(r, EventResponse::Handled) { self.sync_after_nav(); }
                return r;
            }
            // Auto-focus pane on click
            let le = self.sidebar_width + self.list_width;
            if self.sidebar_width > 0.0 && *x < self.sidebar_width { self.focus = PaneFocus::Sidebar; }
            else if *x < le { self.focus = PaneFocus::FileList; }
            else if self.preview_width > 0.0 { self.focus = PaneFocus::Preview; }
            // Route to focused pane
            let pane_y = adj_y - BREADCRUMB_HEIGHT;
            let adj_x = if self.focus == PaneFocus::Sidebar { *x } else { *x - self.sidebar_width };
            let adj = WidgetEvent::PointerPress { x: adj_x, y: pane_y, button: *button, modifiers: *modifiers };
            let r = match self.focus {
                PaneFocus::Sidebar => self.sidebar.event(&adj),
                PaneFocus::FileList => self.file_list.event(&adj),
                PaneFocus::Preview => EventResponse::Ignored,
            };
            if matches!(r, EventResponse::Handled) { self.sync_after_nav(); }
            return r;
        }

        // Route scroll by last pointer x position
        if let WidgetEvent::Scroll { .. } = event {
            let list_end = self.sidebar_width + self.list_width;
            if self.last_pointer_x >= list_end {
                return self.preview.event(event);
            }
            let result = self.file_list.event(event);
            if matches!(result, EventResponse::Handled) {
                self.preview.update_preview();
                self.update_status();
            }
            return result;
        }

        // Tab shortcuts: Ctrl+T/W/Tab/1-9
        if let WidgetEvent::Key(ke) = event {
            let action = self.tab_bar.handle_key(ke);
            if !matches!(action, crate::tab_bar::TabAction::None) {
                self.execute_tab_action(action);
                return EventResponse::Handled;
            }
            use hayate_ui::platform::keyboard::KeyState;
            if ke.state == KeyState::Pressed {
                // F6: cycle pane focus
                if ke.keysym == xkbcommon::xkb::Keysym::F6 {
                    self.focus = self.next_focus(ke.modifiers.shift);
                    return EventResponse::Handled;
                }
                // F12: toggle terminal
                if ke.keysym == xkbcommon::xkb::Keysym::F12 {
                    self.terminal_visible = !self.terminal_visible;
                    if self.terminal_visible && !self.terminal.is_active() {
                        self.terminal.spawn(&self.state.borrow().current_path);
                    }
                    return EventResponse::Handled;
                }
            }
            // Route keyboard to terminal when visible and it's a key event
            if self.terminal_visible {
                let r = self.terminal.event(event);
                if matches!(r, EventResponse::Handled) { return r; }
            }
        }

        // Breadcrumb first (Ctrl+L etc.)
        let bc_result = self.breadcrumb.event(event);
        if matches!(bc_result, EventResponse::Handled) { return bc_result; }

        // Route keyboard to focused pane
        let result = match self.focus {
            PaneFocus::Sidebar => self.sidebar.event(event),
            PaneFocus::FileList => self.file_list.event(event),
            PaneFocus::Preview => self.preview.event(event),
        };
        if matches!(result, EventResponse::Handled) { self.sync_after_nav(); }
        result
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        self.file_list.dirty() || self.sidebar.dirty() || self.preview.dirty()
            || self.toast.borrow().has_visible()
            || (self.terminal_visible && self.terminal.dirty())
    }

    fn clear_dirty(&mut self) {
        self.file_list.clear_dirty();
        self.sidebar.clear_dirty();
        self.preview.clear_dirty();
    }
}

fn tight(w: f32, h: f32) -> Constraints {
    Constraints { min_width: w, max_width: w, min_height: h, max_height: h }
}

fn next_focus_logic(cur: PaneFocus, has_sb: bool, has_pv: bool, rev: bool) -> PaneFocus {
    let vis: Vec<PaneFocus> = [
        (has_sb, PaneFocus::Sidebar), (true, PaneFocus::FileList), (has_pv, PaneFocus::Preview),
    ].iter().filter(|(v, _)| *v).map(|(_, f)| *f).collect();
    let i = vis.iter().position(|f| *f == cur).unwrap_or(0);
    let n = if rev { (i + vis.len() - 1) % vis.len() } else { (i + 1) % vis.len() };
    vis[n]
}

#[cfg(test)]
#[path = "three_pane_tests.rs"]
mod tests;
