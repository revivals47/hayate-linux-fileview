use std::cell::RefCell;
use std::rc::Rc;

use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_ui::widget::text_widget::RichTextWidget;

use crate::state::FileViewState;

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

const TITLE_FONT: f32 = 14.0;
const CONTENT_FONT: f32 = 12.0;
const INFO_FONT: f32 = 11.0;
const PADDING: f32 = 10.0;
const TITLE_HEIGHT: f32 = 20.0;
const INFO_HEIGHT: f32 = 16.0;

const SCROLL_STEP: f32 = 16.0;

pub struct PreviewPane {
    state: Rc<RefCell<FileViewState>>,
    title_widget: RichTextWidget,
    content_widget: RichTextWidget,
    info_widget: RichTextWidget,
    width: f32,
    height: f32,
    scroll_offset: f32,
}

impl PreviewPane {
    pub fn new(state: Rc<RefCell<FileViewState>>, engine: Rc<RefCell<TextEngine>>) -> Self {
        let title_widget = RichTextWidget::new("No selection", TITLE_FONT)
            .with_engine(engine.clone())
            .with_color(140, 180, 240);
        let content_widget = RichTextWidget::new("Select a file to preview", CONTENT_FONT)
            .with_engine(engine.clone())
            .with_color(180, 180, 180);
        let info_widget = RichTextWidget::new("", INFO_FONT)
            .with_engine(engine.clone())
            .with_color(120, 120, 120);
        Self {
            state,
            title_widget,
            content_widget,
            info_widget,
            width: 250.0,
            height: 400.0,
            scroll_offset: 0.0,
        }
    }

    pub fn update_preview(&mut self) {
        // Extract needed data from state, then drop the borrow before
        // calling &mut self methods (load_file_preview).
        let (selected_info, path) = {
            let state = self.state.borrow();
            match state.selected_index {
                None => (None, None),
                Some(idx) if idx >= state.entries.len() => (None, None),
                Some(idx) => {
                    let entry = &state.entries[idx];
                    let name = entry.name.clone();
                    let is_dir = entry.is_dir;
                    let path = state.current_path.join(&name);
                    (Some((name, is_dir)), Some(path))
                }
            }
        };

        self.scroll_offset = 0.0;

        match (selected_info, path) {
            (None, _) => {
                self.title_widget.set_text("No selection");
                self.content_widget.set_text("Select a file to preview");
                self.info_widget.set_text("");
            }
            (Some((name, is_dir)), Some(path)) => {
                self.title_widget.set_text(&name);

                if is_dir {
                    let count = std::fs::read_dir(&path)
                        .map(|rd| rd.count())
                        .unwrap_or(0);
                    self.content_widget
                        .set_text(&format!("Directory: {} items", count));
                } else {
                    self.load_file_preview(&path);
                }

                if let Ok(meta) = std::fs::metadata(&path) {
                    self.info_widget.set_text(&format!(
                        "Size: {}  Type: {}  Readonly: {}",
                        format_bytes(meta.len()),
                        if meta.is_dir() { "Directory" } else { "File" },
                        meta.permissions().readonly()
                    ));
                }
            }
            _ => {}
        }
    }

    fn load_file_preview(&mut self, path: &std::path::Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        match ext.as_str() {
            "txt" | "md" | "rs" | "toml" | "json" | "yaml" | "yml" | "py" | "js" | "ts"
            | "sh" | "html" | "css" | "c" | "h" | "cpp" | "hpp" | "go" | "java" | "xml"
            | "cfg" | "ini" | "log" | "csv" => match std::fs::read_to_string(path) {
                Ok(s) => {
                    let preview: String = s.chars().take(16384).collect();
                    let lines: Vec<&str> = preview.lines().take(100).collect();
                    self.content_widget.set_text(&lines.join("\n"));
                }
                Err(e) => {
                    self.content_widget.set_text(&format!("Error: {}", e));
                }
            },
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => {
                self.content_widget.set_text("🖼  Image file");
            }
            _ => {
                self.content_widget.set_text("Binary file");
            }
        }
    }
}

impl Widget for PreviewPane {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.width = constraints.max_width;
        self.height = constraints.max_height;

        let text_width = self.width - PADDING * 2.0;
        let text_constraints = Constraints {
            min_width: 0.0,
            max_width: text_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        self.title_widget.layout(&text_constraints);
        self.content_widget.layout(&text_constraints);
        self.info_widget.layout(&text_constraints);

        Size::new(self.width, self.height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        // Fill background with dark color (R=30, G=30, B=35) in BGRA byte order
        let x0 = rect.x.max(0.0) as u32;
        let y0 = rect.y.max(0.0) as u32;
        let x1 = (rect.x + rect.width) as u32;
        let y1 = (rect.y + rect.height) as u32;
        for py in y0..y1 {
            for px in x0..x1 {
                let offset = (py * stride + px * 4) as usize;
                if offset + 3 < canvas.len() {
                    canvas[offset] = 35; // B
                    canvas[offset + 1] = 30; // G
                    canvas[offset + 2] = 30; // R
                    canvas[offset + 3] = 255; // A
                }
            }
        }

        // Title
        let title_rect = ItemRect::new(
            rect.x + PADDING,
            rect.y + PADDING,
            self.width - PADDING * 2.0,
            TITLE_HEIGHT,
        );
        self.title_widget.paint(canvas, title_rect, stride);

        // Info line
        let info_rect = ItemRect::new(
            rect.x + PADDING,
            rect.y + PADDING + TITLE_HEIGHT,
            self.width - PADDING * 2.0,
            INFO_HEIGHT,
        );
        self.info_widget.paint(canvas, info_rect, stride);

        // Content (shifted by scroll_offset)
        let content_y = rect.y + PADDING + TITLE_HEIGHT + INFO_HEIGHT + 4.0 - self.scroll_offset;
        let content_height = (rect.height - (PADDING + TITLE_HEIGHT + INFO_HEIGHT + 4.0)).max(0.0)
            + self.scroll_offset;
        let content_rect = ItemRect::new(
            rect.x + PADDING,
            content_y,
            self.width - PADDING * 2.0,
            content_height,
        );
        self.content_widget.paint(canvas, content_rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        if let WidgetEvent::Scroll { dy, .. } = event {
            self.scroll_offset = (self.scroll_offset + *dy * SCROLL_STEP).max(0.0);
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn dirty(&self) -> bool {
        true
    }
}
