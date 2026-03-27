//! Status bar widget displayed at the bottom of the file viewer window.

use std::cell::RefCell;
use std::rc::Rc;

use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

const BAR_HEIGHT: f32 = 20.0;

fn format_size(bytes: u64) -> String {
    if bytes < 1024 { format!("{} B", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) }
    else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
}
const BG_COLOR: (u8, u8, u8) = (40, 40, 45);

/// Information needed to render the status bar.
pub struct StatusInfo<'a> {
    pub item_count: usize,
    pub show_hidden: bool,
    pub selected_name: Option<&'a str>,
    pub selected_size: Option<String>,
    pub selected_count: usize,
    pub selected_total_size: u64,
    pub current_path: &'a std::path::Path,
}

pub struct StatusBar {
    message: String,
    engine: Rc<RefCell<TextEngine>>,
}

impl StatusBar {
    pub fn new(engine: Rc<RefCell<TextEngine>>) -> Self {
        Self {
            message: String::new(),
            engine,
        }
    }

    pub fn update(&mut self, info: &StatusInfo<'_>) {
        let hidden = if info.show_hidden { "on" } else { "off" };
        let selected = if info.selected_count > 1 {
            format!(" | {} selected ({})", info.selected_count, format_size(info.selected_total_size))
        } else {
            match (info.selected_name, &info.selected_size) {
                (Some(name), Some(size)) => format!(" | {}: {}", name, size),
                (Some(name), None) => format!(" | {}/", name),
                _ => String::new(),
            }
        };
        self.message = format!(
            "  {} items [hidden: {}]{} — {}",
            info.item_count, hidden, selected, info.current_path.display(),
        );
    }
}

impl Widget for StatusBar {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        Size::new(constraints.max_width, BAR_HEIGHT)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        // Fill background
        let (bg_r, bg_g, bg_b) = BG_COLOR;
        let y_start = rect.y.max(0.0) as u32;
        let y_end = (rect.y + rect.height).min((stride / 4) as f32) as u32;
        let x_start = rect.x.max(0.0) as u32;
        let x_end = (rect.x + rect.width) as u32;
        let canvas_height = canvas.len() as u32 / stride;

        for py in y_start..y_end.min(canvas_height) {
            for px in x_start..x_end {
                let offset = (py * stride + px * 4) as usize;
                if offset + 3 < canvas.len() {
                    canvas[offset] = bg_b;
                    canvas[offset + 1] = bg_g;
                    canvas[offset + 2] = bg_r;
                    canvas[offset + 3] = 255;
                }
            }
        }

        // Render text on top of background
        if !self.message.is_empty() {
            use hayate_ui::widget::text_widget::RichTextWidget;
            let mut text = RichTextWidget::new(self.message.clone(), 11.0)
                .with_engine(self.engine.clone())
                .with_color(180, 180, 180);
            let text_constraints = Constraints::tight(rect.width, BAR_HEIGHT);
            text.layout(&text_constraints);
            text.paint(canvas, rect, stride);
        }
    }

    fn event(&mut self, _event: &WidgetEvent) -> EventResponse {
        EventResponse::Ignored
    }

    fn dirty(&self) -> bool {
        true
    }
}
