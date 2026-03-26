use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::app::App;
use hayate_ui::render::TextEngine;
use hayate_ui::widget::layout::{Padding, VStack};
use hayate_ui::widget::text_widget::RichTextWidget;
use hayate_ui::widget::core::Widget;

/// Read directory entries, sorted: directories first, then by name.
fn read_dir_sorted(path: &std::path::Path) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push(DirEntry { name, is_dir, size });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

struct DirEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

impl DirEntry {
    fn display_line(&self) -> String {
        if self.is_dir {
            format!("📁  {}/", self.name)
        } else {
            let size = if self.size < 1024 {
                format!("{} B", self.size)
            } else if self.size < 1024 * 1024 {
                format!("{:.1} KB", self.size as f64 / 1024.0)
            } else {
                format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
            };
            format!("    {}  —  {}", self.name, size)
        }
    }
}

fn build_file_list(path: &std::path::Path, engine: Rc<RefCell<TextEngine>>) -> Box<dyn Widget> {
    let entries = read_dir_sorted(path);
    let mut vstack = VStack::new(1.0);

    // Header
    let header = RichTextWidget::new(
        format!("  {}", path.display()), 16.0,
    ).with_engine(engine.clone())
     .with_color(100, 180, 255);
    vstack.push(Box::new(header));

    // Separator
    let sep = RichTextWidget::new(
        "─".repeat(80), 8.0,
    ).with_engine(engine.clone())
     .with_color(60, 60, 60);
    vstack.push(Box::new(sep));

    // Entries
    for entry in &entries {
        let (r, g, b) = if entry.is_dir {
            (220, 180, 80)
        } else {
            (190, 190, 190)
        };
        let w = RichTextWidget::new(entry.display_line(), 13.0)
            .with_engine(engine.clone())
            .with_color(r, g, b);
        vstack.push(Box::new(w));
    }

    // Footer
    let footer = RichTextWidget::new(
        format!("  {} items", entries.len()), 11.0,
    ).with_engine(engine.clone())
     .with_color(100, 100, 100);
    vstack.push(Box::new(footer));

    Box::new(Padding::all(12.0, Box::new(vstack)))
}

fn main() {
    let path = env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    let engine = Rc::new(RefCell::new(TextEngine::new()));
    let root = build_file_list(&path, engine);

    let title = format!("Hayate — {}", path.display());
    if let Err(e) = App::new(title, 900, 700).run(root) {
        eprintln!("Error: {e}");
    }
}
