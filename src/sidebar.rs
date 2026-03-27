//! Sidebar widget with bookmarks and mounted devices.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_ui::widget::text_widget::RichTextWidget;

use crate::state::FileViewState;

const SIDEBAR_WIDTH: f32 = 150.0;
const SECTION_FONT: f32 = 12.0;
const ENTRY_FONT: f32 = 12.0;
const ROW_HEIGHT: f32 = 18.0;
const PADDING: f32 = 8.0;
const BG_COLOR: (u8, u8, u8) = (25, 25, 30);

struct SidebarEntry {
    label: String,
    path: PathBuf,
    icon: &'static str,
}

pub(crate) struct SidebarWidget {
    state: Rc<RefCell<FileViewState>>,
    engine: Rc<RefCell<TextEngine>>,
    bookmarks: Vec<SidebarEntry>,
    mounts: Vec<SidebarEntry>,
    width: f32,
    height: f32,
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

fn default_bookmarks() -> Vec<SidebarEntry> {
    let home = home_dir();
    vec![
        SidebarEntry {
            label: "Home".into(),
            path: home.clone(),
            icon: "🏠",
        },
        SidebarEntry {
            label: "Documents".into(),
            path: home.join("Documents"),
            icon: "📄",
        },
        SidebarEntry {
            label: "Downloads".into(),
            path: home.join("Downloads"),
            icon: "📥",
        },
        SidebarEntry {
            label: "Desktop".into(),
            path: home.join("Desktop"),
            icon: "🖥",
        },
        SidebarEntry {
            label: "Root".into(),
            path: PathBuf::from("/"),
            icon: "/",
        },
    ]
}

fn read_mounts() -> Vec<SidebarEntry> {
    std::fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let mountpoint = parts.get(1)?;
            if mountpoint.starts_with("/media/") || mountpoint.starts_with("/mnt/") {
                Some(SidebarEntry {
                    label: mountpoint.rsplit('/').next()?.to_string(),
                    path: PathBuf::from(mountpoint),
                    icon: "💾",
                })
            } else {
                None
            }
        })
        .collect()
}

impl SidebarWidget {
    pub(crate) fn new(
        state: Rc<RefCell<FileViewState>>,
        engine: Rc<RefCell<TextEngine>>,
    ) -> Self {
        let bookmarks = default_bookmarks();
        let mounts = read_mounts();
        Self {
            state,
            engine,
            bookmarks,
            mounts,
            width: SIDEBAR_WIDTH,
            height: 400.0,
        }
    }

    /// Map click Y to (section, index within section).
    /// Returns None for header rows, Some(path) for entry rows.
    fn y_to_path(&self, y: f32) -> Option<PathBuf> {
        if y < PADDING {
            return None;
        }
        let row = ((y - PADDING) / ROW_HEIGHT) as usize;

        // Row 0 = "Bookmarks" header
        if row == 0 {
            return None;
        }
        // Rows 1..=bookmarks.len() = bookmark entries
        let bm_end = 1 + self.bookmarks.len();
        if row < bm_end {
            return Some(self.bookmarks[row - 1].path.clone());
        }
        // Row bm_end = "Devices" header
        if row == bm_end {
            return None;
        }
        // Rows after = mount entries
        let mt_idx = row - bm_end - 1;
        if mt_idx < self.mounts.len() {
            return Some(self.mounts[mt_idx].path.clone());
        }
        None
    }

    fn build_widgets(&self) -> Vec<Box<dyn Widget>> {
        let mut widgets: Vec<Box<dyn Widget>> = Vec::new();

        // Bookmarks header
        let header = RichTextWidget::new("Bookmarks", SECTION_FONT)
            .with_engine(self.engine.clone())
            .with_color(100, 160, 220);
        widgets.push(Box::new(header));

        // Bookmark entries
        for entry in &self.bookmarks {
            let exists = entry.path.exists();
            let (r, g, b) = if exists { (190, 190, 190) } else { (80, 80, 80) };
            let label = format!("{} {}", entry.icon, entry.label);
            let w = RichTextWidget::new(label, ENTRY_FONT)
                .with_engine(self.engine.clone())
                .with_color(r, g, b);
            widgets.push(Box::new(w));
        }

        // Devices header
        let dev_header = RichTextWidget::new("Devices", SECTION_FONT)
            .with_engine(self.engine.clone())
            .with_color(100, 160, 220);
        widgets.push(Box::new(dev_header));

        // Mount entries
        if self.mounts.is_empty() {
            let empty = RichTextWidget::new("  (none)", ENTRY_FONT)
                .with_engine(self.engine.clone())
                .with_color(80, 80, 80);
            widgets.push(Box::new(empty));
        } else {
            for entry in &self.mounts {
                let label = format!("{} {}", entry.icon, entry.label);
                let w = RichTextWidget::new(label, ENTRY_FONT)
                    .with_engine(self.engine.clone())
                    .with_color(190, 190, 190);
                widgets.push(Box::new(w));
            }
        }

        widgets
    }
}

impl Widget for SidebarWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.width = SIDEBAR_WIDTH.min(constraints.max_width);
        self.height = constraints.max_height;
        Size::new(self.width, self.height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        // Fill background
        let (bg_r, bg_g, bg_b) = BG_COLOR;
        let x0 = rect.x.max(0.0) as u32;
        let y0 = rect.y.max(0.0) as u32;
        let x1 = (rect.x + rect.width) as u32;
        let y1 = (rect.y + rect.height) as u32;
        for py in y0..y1 {
            for px in x0..x1 {
                let offset = (py * stride + px * 4) as usize;
                if offset + 3 < canvas.len() {
                    canvas[offset] = bg_b;
                    canvas[offset + 1] = bg_g;
                    canvas[offset + 2] = bg_r;
                    canvas[offset + 3] = 255;
                }
            }
        }

        // Paint text rows
        let widgets = self.build_widgets();
        for (i, w) in widgets.iter().enumerate() {
            let row_y = rect.y + PADDING + i as f32 * ROW_HEIGHT;
            if row_y + ROW_HEIGHT > rect.y + rect.height {
                break;
            }
            let row_rect = ItemRect::new(
                rect.x + PADDING,
                row_y,
                self.width - PADDING * 2.0,
                ROW_HEIGHT,
            );
            w.paint(canvas, row_rect, stride);
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::PointerPress { y, button: 0x110, .. } => {
                if let Some(path) = self.y_to_path(*y) {
                    if path.is_dir() {
                        self.state.borrow_mut().navigate(path);
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn dirty(&self) -> bool {
        true
    }
}
