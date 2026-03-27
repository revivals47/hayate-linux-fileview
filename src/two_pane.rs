//! Two-pane layout: file list on the left, preview on the right.

use std::cell::RefCell;
use std::rc::Rc;

use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::file_list::FileListWidget;
use crate::preview::PreviewPane;
use crate::state::FileViewState;

pub(crate) struct TwoPaneWidget {
    file_list: FileListWidget,
    preview: PreviewPane,
    list_width: f32,
    preview_width: f32,
}

impl TwoPaneWidget {
    pub(crate) fn new(state: Rc<RefCell<FileViewState>>, engine: Rc<RefCell<TextEngine>>) -> Self {
        let file_list = FileListWidget::new(Rc::clone(&state));
        let preview = PreviewPane::new(Rc::clone(&state), engine);
        Self {
            file_list,
            preview,
            list_width: 0.0,
            preview_width: 0.0,
        }
    }
}

impl Widget for TwoPaneWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.preview_width = 250.0_f32.min(constraints.max_width * 0.4);
        self.list_width = constraints.max_width - self.preview_width;

        let list_constraints = Constraints {
            min_width: self.list_width,
            max_width: self.list_width,
            min_height: constraints.min_height,
            max_height: constraints.max_height,
        };
        self.file_list.layout(&list_constraints);

        let preview_constraints = Constraints {
            min_width: self.preview_width,
            max_width: self.preview_width,
            min_height: constraints.min_height,
            max_height: constraints.max_height,
        };
        self.preview.layout(&preview_constraints);

        Size::new(constraints.max_width, constraints.max_height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let list_rect = ItemRect::new(rect.x, rect.y, self.list_width, rect.height);
        self.file_list.paint(canvas, list_rect, stride);

        let preview_rect =
            ItemRect::new(rect.x + self.list_width, rect.y, self.preview_width, rect.height);
        self.preview.paint(canvas, preview_rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::PointerPress { x, .. } if *x >= self.list_width => {
                return EventResponse::Ignored;
            }
            _ => {}
        }

        let result = self.file_list.event(event);
        if matches!(result, EventResponse::Handled) {
            self.preview.update_preview();
        }
        result
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        true
    }
}
