//! Three-pane layout: sidebar | file list | preview, with a status bar.

use std::cell::RefCell;
use std::rc::Rc;

use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::breadcrumb::BreadcrumbWidget;
use crate::file_list::FileListWidget;
use crate::preview::PreviewPane;
use crate::sidebar::SidebarWidget;
use crate::state::FileViewState;
use crate::status_bar::{StatusBar, StatusInfo};

const BREADCRUMB_HEIGHT: f32 = 24.0;
const STATUS_HEIGHT: f32 = 20.0;

pub(crate) struct ThreePaneWidget {
    breadcrumb: BreadcrumbWidget,
    sidebar: SidebarWidget,
    file_list: FileListWidget,
    preview: PreviewPane,
    status_bar: StatusBar,
    sidebar_width: f32,
    list_width: f32,
    preview_width: f32,
    last_pointer_x: f32,
}

impl ThreePaneWidget {
    pub(crate) fn new(
        state: Rc<RefCell<FileViewState>>,
        engine: Rc<RefCell<TextEngine>>,
    ) -> Self {
        let breadcrumb = BreadcrumbWidget::new(Rc::clone(&state), engine.clone());
        let sidebar = SidebarWidget::new(Rc::clone(&state), engine.clone());
        let file_list = FileListWidget::new(Rc::clone(&state));
        let preview = PreviewPane::new(Rc::clone(&state), engine.clone());
        let mut status_bar = StatusBar::new(engine);
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
            });
        }
        Self {
            breadcrumb,
            sidebar,
            file_list,
            preview,
            status_bar,
            sidebar_width: 0.0,
            list_width: 0.0,
            preview_width: 0.0,
            last_pointer_x: 0.0,
        }
    }

    fn update_status(&mut self) {
        let state = self.file_list.state().borrow();
        let selected_count = state.selected.len();
        let selected_total_size: u64 = state.selected.iter()
            .filter_map(|&i| state.entries.get(i))
            .map(|e| e.size)
            .sum();
        let (selected_name, selected_size) = match state.cursor {
            Some(idx) if idx < state.entries.len() => {
                let e = &state.entries[idx];
                let size = if e.is_dir { None } else { Some(e.format_size()) };
                (Some(e.name.as_str()), size)
            }
            _ => (None, None),
        };
        self.status_bar.update(&StatusInfo {
            item_count: state.entries.len(),
            show_hidden: state.show_hidden,
            selected_name,
            selected_size,
            selected_count,
            selected_total_size,
            current_path: &state.current_path,
        });
    }
}

impl Widget for ThreePaneWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        let total_width = constraints.max_width;
        let content_height = constraints.max_height - BREADCRUMB_HEIGHT - STATUS_HEIGHT;

        // Breadcrumb bar (full width, top)
        let bc_c = Constraints {
            min_width: total_width,
            max_width: total_width,
            min_height: BREADCRUMB_HEIGHT,
            max_height: BREADCRUMB_HEIGHT,
        };
        self.breadcrumb.layout(&bc_c);

        self.sidebar_width = 150.0_f32.min(total_width * 0.2);
        let remaining = total_width - self.sidebar_width;
        self.preview_width = 250.0_f32.min(remaining * 0.35);
        self.list_width = remaining - self.preview_width;

        let sidebar_c = Constraints {
            min_width: self.sidebar_width,
            max_width: self.sidebar_width,
            min_height: content_height,
            max_height: content_height,
        };
        self.sidebar.layout(&sidebar_c);

        let list_c = Constraints {
            min_width: self.list_width,
            max_width: self.list_width,
            min_height: content_height,
            max_height: content_height,
        };
        self.file_list.layout(&list_c);

        let preview_c = Constraints {
            min_width: self.preview_width,
            max_width: self.preview_width,
            min_height: content_height,
            max_height: content_height,
        };
        self.preview.layout(&preview_c);

        let status_c = Constraints {
            min_width: total_width,
            max_width: total_width,
            min_height: STATUS_HEIGHT,
            max_height: STATUS_HEIGHT,
        };
        self.status_bar.layout(&status_c);

        Size::new(total_width, constraints.max_height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let content_height = rect.height - BREADCRUMB_HEIGHT - STATUS_HEIGHT;
        let pane_y = rect.y + BREADCRUMB_HEIGHT;

        // Breadcrumb bar (full width, top)
        let bc_rect = ItemRect::new(rect.x, rect.y, rect.width, BREADCRUMB_HEIGHT);
        self.breadcrumb.paint(canvas, bc_rect, stride);

        // Sidebar
        let sidebar_rect =
            ItemRect::new(rect.x, pane_y, self.sidebar_width, content_height);
        self.sidebar.paint(canvas, sidebar_rect, stride);

        // File list
        let list_x = rect.x + self.sidebar_width;
        let list_rect = ItemRect::new(list_x, pane_y, self.list_width, content_height);
        self.file_list.paint(canvas, list_rect, stride);

        // Preview
        let preview_x = list_x + self.list_width;
        let preview_rect =
            ItemRect::new(preview_x, pane_y, self.preview_width, content_height);
        self.preview.paint(canvas, preview_rect, stride);

        // Status bar (full width, bottom)
        let status_y = pane_y + content_height;
        let status_rect =
            ItemRect::new(rect.x, status_y, rect.width, STATUS_HEIGHT);
        self.status_bar.paint(canvas, status_rect, stride);
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

        // Route pointer press by y then x coordinate
        if let WidgetEvent::PointerPress { x, y, button, modifiers, .. } = event {
            // Breadcrumb bar (top strip)
            if *y < BREADCRUMB_HEIGHT {
                let result = self.breadcrumb.event(event);
                if matches!(result, EventResponse::Handled) {
                    self.file_list.rebuild();
                    self.preview.update_preview();
                    self.update_status();
                }
                return result;
            }
            // Adjust y for pane area (below breadcrumb)
            let pane_y = *y - BREADCRUMB_HEIGHT;
            if *x < self.sidebar_width {
                let adjusted = WidgetEvent::PointerPress {
                    x: *x,
                    y: pane_y,
                    button: *button,
                    modifiers: *modifiers,
                };
                let result = self.sidebar.event(&adjusted);
                if matches!(result, EventResponse::Handled) {
                    self.breadcrumb.update_segments();
                    self.file_list.rebuild();
                    self.preview.update_preview();
                    self.update_status();
                }
                return result;
            }
            let list_end = self.sidebar_width + self.list_width;
            if *x < list_end {
                let adjusted = WidgetEvent::PointerPress {
                    x: *x - self.sidebar_width,
                    y: pane_y,
                    button: *button,
                    modifiers: *modifiers,
                };
                let result = self.file_list.event(&adjusted);
                if matches!(result, EventResponse::Handled) {
                    self.breadcrumb.update_segments();
                    self.preview.update_preview();
                    self.update_status();
                }
                return result;
            }
            return EventResponse::Ignored;
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

        // Keyboard events → breadcrumb first (Ctrl+L), then file list
        let bc_result = self.breadcrumb.event(event);
        if matches!(bc_result, EventResponse::Handled) {
            return bc_result;
        }
        let result = self.file_list.event(event);
        if matches!(result, EventResponse::Handled) {
            self.breadcrumb.update_segments();
            self.preview.update_preview();
            self.update_status();
        }
        result
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        self.file_list.dirty() || self.sidebar.dirty() || self.preview.dirty()
    }

    fn clear_dirty(&mut self) {
        self.file_list.clear_dirty();
        self.sidebar.clear_dirty();
        self.preview.clear_dirty();
    }
}
