use std::cell::RefCell;
use std::rc::Rc;

use hayate_platform::render::{Renderer, TextEngine};
use hayate_platform::scroll::delegate::ItemRect;
use hayate_platform::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_kit::widget::text_widget::RichTextWidget;
use hayate_platform::widget::widget_id::alloc_widget_id;
use hayate_platform::widget::focus::WidgetId;

use crate::entry::format_size;
use crate::state::FileViewState;

const TITLE_FONT: f32 = 14.0;
const CONTENT_FONT: f32 = 12.0;
const INFO_FONT: f32 = 11.0;
const PADDING: f32 = 10.0;
const TITLE_HEIGHT: f32 = 20.0;
const INFO_HEIGHT: f32 = 16.0;
const SCROLL_STEP: f32 = 16.0;
/// Max decoded image dimension before scaling (saves memory).
const MAX_DECODE_DIM: u32 = 2048;

/// Scaled image ready for blitting to the canvas.
struct ImagePreview {
    /// BGRA pixel data, pre-scaled to fit preview pane.
    bgra: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct PreviewPane {
    /// Stable widget identity (Phase 5: `Widget::id` is required).
    id: WidgetId,
    state: Rc<RefCell<FileViewState>>,
    title_widget: RichTextWidget,
    content_widget: RichTextWidget,
    info_widget: RichTextWidget,
    image: Option<ImagePreview>,
    width: f32,
    height: f32,
    scroll_offset: f32,
    pub(crate) is_dirty: bool,
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
            id: alloc_widget_id(),
            state,
            title_widget,
            content_widget,
            info_widget,
            image: None,
            width: 250.0,
            height: 400.0,
            scroll_offset: 0.0,
            is_dirty: true,
        }
    }

    pub fn update_preview(&mut self) {
        self.is_dirty = true;
        // Extract needed data from state, then drop the borrow before
        // calling &mut self methods (load_file_preview).
        let (selected_info, path) = {
            let state = self.state.borrow();
            match state.cursor {
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
        self.image = None;

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
                        format_size(meta.len()),
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
            "png" | "jpg" | "jpeg" => {
                self.load_image(path);
            }
            "gif" | "bmp" | "svg" | "webp" => {
                self.content_widget.set_text("Image file (format not yet supported)");
            }
            _ => {
                self.content_widget.set_text("Binary file");
            }
        }
    }

    fn load_image(&mut self, path: &std::path::Path) {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => { self.content_widget.set_text(&format!("Error: {e}")); return; }
        };
        match decode_image(&data) {
            Some((rgba, w, h)) => self.set_image_from_rgba(&rgba, w, h),
            None => {
                let size = format_size(data.len() as u64);
                self.content_widget.set_text(&format!(
                    "Image: {}\nSize: {size}\n(Preview not available)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                ));
            }
        }
    }

    /// Scale RGBA image data and convert to BGRA for the canvas.
    fn set_image_from_rgba(&mut self, rgba: &[u8], iw: u32, ih: u32) {
        if iw > MAX_DECODE_DIM || ih > MAX_DECODE_DIM {
            self.content_widget.set_text(&format!("Image too large: {iw}x{ih}"));
            return;
        }
        let avail_w = (self.width - PADDING * 2.0).max(1.0);
        let avail_h = (self.height - PADDING - TITLE_HEIGHT - INFO_HEIGHT - 8.0).max(1.0);
        let scale = (avail_w / iw as f32).min(avail_h / ih as f32).min(1.0);
        let tw = (iw as f32 * scale).max(1.0) as u32;
        let th = (ih as f32 * scale).max(1.0) as u32;
        let mut bgra = vec![0u8; (tw * th * 4) as usize];
        for dy in 0..th {
            for dx in 0..tw {
                let sx = ((dx as f32 + 0.5) / scale) as u32;
                let sy = ((dy as f32 + 0.5) / scale) as u32;
                let si = ((sy.min(ih - 1)) * iw + sx.min(iw - 1)) as usize * 4;
                let di = (dy * tw + dx) as usize * 4;
                if si + 3 < rgba.len() && di + 3 < bgra.len() {
                    bgra[di] = rgba[si + 2];     // B ← R
                    bgra[di + 1] = rgba[si + 1]; // G
                    bgra[di + 2] = rgba[si];     // R ← B
                    bgra[di + 3] = rgba[si + 3]; // A
                }
            }
        }
        self.content_widget.set_text(&format!("{iw}x{ih}"));
        self.image = Some(ImagePreview { bgra, width: tw, height: th });
    }
}

/// Decode an image (JPEG/PNG) via the `image` crate. Returns RGBA pixels + dimensions.
fn decode_image(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = match image::load_from_memory(data) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[preview] Image decode error: {e}");
            // Fallback: try guessing format explicitly
            return decode_image_explicit(data);
        }
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), w, h))
}

/// Explicit format decode fallback (handles cases where magic bytes fail).
fn decode_image_explicit(data: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    // Try JPEG explicitly (handles non-standard headers)
    let cursor = std::io::Cursor::new(data);
    let reader = image::ImageReader::new(cursor)
        .with_guessed_format().ok()?;
    let img = reader.decode().ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), w, h))
}

impl Widget for PreviewPane {
    fn id(&self) -> WidgetId {
        self.id
    }

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

    fn paint(&mut self, renderer: &mut Renderer, rect: ItemRect) {
        if let Some((canvas, stride)) = renderer.pixels_mut() {
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

            // Content area — image blit (needs canvas directly)
            let content_y = rect.y + PADDING + TITLE_HEIGHT + INFO_HEIGHT + 4.0;
            if let Some(ref img) = self.image {
                let avail_w = self.width - PADDING * 2.0;
                let offset_x = ((avail_w - img.width as f32) / 2.0).max(0.0);
                let bx = (rect.x + PADDING + offset_x) as u32;
                let by = content_y as u32;
                for dy in 0..img.height {
                    let canvas_y = by + dy;
                    for dx in 0..img.width {
                        let si = (dy * img.width + dx) as usize * 4;
                        let co = (canvas_y * stride + (bx + dx) * 4) as usize;
                        if co + 3 < canvas.len() {
                            canvas[co] = img.bgra[si];
                            canvas[co + 1] = img.bgra[si + 1];
                            canvas[co + 2] = img.bgra[si + 2];
                            canvas[co + 3] = img.bgra[si + 3];
                        }
                    }
                }
            }
        }

        // Title (uses Widget::paint with Renderer)
        let title_rect = ItemRect::new(
            rect.x + PADDING,
            rect.y + PADDING,
            self.width - PADDING * 2.0,
            TITLE_HEIGHT,
        );
        self.title_widget.paint(renderer, title_rect);

        // Info line
        let info_rect = ItemRect::new(
            rect.x + PADDING,
            rect.y + PADDING + TITLE_HEIGHT,
            self.width - PADDING * 2.0,
            INFO_HEIGHT,
        );
        self.info_widget.paint(renderer, info_rect);

        // Content text (only when no image)
        let content_y = rect.y + PADDING + TITLE_HEIGHT + INFO_HEIGHT + 4.0;
        if self.image.is_none() {
            let cy = content_y - self.scroll_offset;
            let ch = (rect.height - (PADDING + TITLE_HEIGHT + INFO_HEIGHT + 4.0)).max(0.0)
                + self.scroll_offset;
            let cr = ItemRect::new(rect.x + PADDING, cy, self.width - PADDING * 2.0, ch);
            self.content_widget.paint(renderer, cr);
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        if let WidgetEvent::Scroll { dy, .. } = event {
            self.scroll_offset = (self.scroll_offset + *dy * SCROLL_STEP).max(0.0);
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn dirty(&self) -> bool {
        self.is_dirty
    }

    fn clear_dirty(&mut self) {
        self.is_dirty = false;
    }
}
