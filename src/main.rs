use std::cell::RefCell;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use xkbcommon::xkb::Keysym;

use hayate_ui::app::App;
use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::render::TextEngine;
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};
use hayate_ui::widget::layout::{Padding, VStack};
use hayate_ui::widget::text_widget::RichTextWidget;

// ── Directory entry ──

struct DirEntry {
    name: String,
    is_dir: bool,
    size: u64,
}

impl DirEntry {
    fn format_size(&self) -> String {
        if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        }
    }

    fn display_line(&self) -> String {
        if self.is_dir {
            format!("📁  {:<50}", format!("{}/", self.name))
        } else {
            format!("    {:<50} {:>10}", self.name, self.format_size())
        }
    }
}

/// Read directory entries, sorted: directories first, then by name.
/// When `show_hidden` is false, entries starting with '.' are skipped.
fn read_dir_sorted(path: &Path, show_hidden: bool) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            entries.push(DirEntry { name, is_dir, size });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

// ── Application state ──

struct FileViewState {
    current_path: PathBuf,
    show_hidden: bool,
    entries: Vec<DirEntry>,
    engine: Rc<RefCell<TextEngine>>,
}

impl FileViewState {
    fn new(path: PathBuf, engine: Rc<RefCell<TextEngine>>) -> Self {
        let show_hidden = false;
        let entries = read_dir_sorted(&path, show_hidden);
        Self {
            current_path: path,
            show_hidden,
            entries,
            engine,
        }
    }

    fn refresh(&mut self) {
        self.entries = read_dir_sorted(&self.current_path, self.show_hidden);
    }

    fn navigate(&mut self, path: PathBuf) {
        self.current_path = path;
        self.refresh();
    }

    fn go_parent(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.navigate(parent);
        }
    }

    fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }
}

// ── ScrollableWidget ──

struct ScrollableWidget {
    inner: Box<dyn Widget>,
    scroll_offset: f32,
    viewport_height: f32,
    content_height: f32,
    line_height: f32,
}

impl ScrollableWidget {
    fn new(inner: Box<dyn Widget>, line_height: f32) -> Self {
        Self {
            inner,
            scroll_offset: 0.0,
            viewport_height: 0.0,
            content_height: 0.0,
            line_height,
        }
    }

    fn scroll_by(&mut self, delta: f32) {
        let max_offset = (self.content_height - self.viewport_height).max(0.0);
        self.scroll_offset = (self.scroll_offset + delta).clamp(0.0, max_offset);
    }

    fn reset_scroll(&mut self) {
        self.scroll_offset = 0.0;
    }
}

impl Widget for ScrollableWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        let inner_constraints = Constraints {
            min_width: constraints.min_width,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        let inner_size = self.inner.layout(&inner_constraints);
        self.content_height = inner_size.height;
        self.viewport_height = constraints.max_height;
        Size::new(inner_size.width, constraints.max_height)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let shifted_rect = ItemRect::new(
            rect.x,
            rect.y - self.scroll_offset,
            rect.width,
            self.content_height,
        );
        self.inner.paint(canvas, shifted_rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                let delta = match ke.keysym {
                    Keysym::Up => -self.line_height,
                    Keysym::Down => self.line_height,
                    Keysym::Page_Up => -self.viewport_height * 0.8,
                    Keysym::Page_Down => self.viewport_height * 0.8,
                    Keysym::Home => -self.scroll_offset,
                    Keysym::End => self.content_height,
                    _ => return self.inner.event(event),
                };
                self.scroll_by(delta);
                EventResponse::Handled
            }
            // Adjust pointer Y for scroll offset before passing to inner
            WidgetEvent::PointerPress { x, y, button } => {
                let adjusted = WidgetEvent::PointerPress {
                    x: *x,
                    y: *y + self.scroll_offset,
                    button: *button,
                };
                self.inner.event(&adjusted)
            }
            _ => self.inner.event(event),
        }
    }

    fn dirty(&self) -> bool {
        true
    }
}

// ── Widget tree builder ──

/// Row layout constants.
/// Header: font 16 + spacing 1 = 17
/// Separator: font 8 + spacing 1 = 9
/// Entry rows (including ".."): font 13 + spacing 1 = 14
const HEADER_HEIGHT: f32 = 17.0;
const SEP_HEIGHT: f32 = 9.0;
const ROW_HEIGHT: f32 = 14.0;
const LIST_PADDING: f32 = 12.0;

fn build_file_list(state: &FileViewState) -> Box<dyn Widget> {
    let engine = &state.engine;
    let mut vstack = VStack::new(1.0);

    // Header: current path + hidden indicator
    let hidden_indicator = if state.show_hidden { " [H]" } else { "" };
    let header = RichTextWidget::new(
        format!("  {}{}", state.current_path.display(), hidden_indicator),
        16.0,
    )
    .with_engine(engine.clone())
    .with_color(100, 180, 255);
    vstack.push(Box::new(header));

    // Separator
    let sep = RichTextWidget::new("─".repeat(80), 8.0)
        .with_engine(engine.clone())
        .with_color(60, 60, 60);
    vstack.push(Box::new(sep));

    // Parent directory entry (..)
    let parent_row = RichTextWidget::new("📁  ../".to_string(), 13.0)
        .with_engine(engine.clone())
        .with_color(150, 150, 220);
    vstack.push(Box::new(parent_row));

    // File/directory entries
    for entry in &state.entries {
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
        format!("  {} items", state.entries.len()),
        11.0,
    )
    .with_engine(engine.clone())
    .with_color(100, 100, 100);
    vstack.push(Box::new(footer));

    Box::new(Padding::all(LIST_PADDING, Box::new(vstack)))
}

// ── FileListWidget: custom widget with event handling ──

struct FileListWidget {
    state: Rc<RefCell<FileViewState>>,
    scroller: ScrollableWidget,
}

impl FileListWidget {
    fn new(state: Rc<RefCell<FileViewState>>) -> Self {
        let inner = build_file_list(&state.borrow());
        let scroller = ScrollableWidget::new(inner, 16.0);
        Self { state, scroller }
    }

    fn rebuild(&mut self) {
        let inner = build_file_list(&self.state.borrow());
        self.scroller.inner = inner;
        self.scroller.reset_scroll();
    }

    /// Convert a click Y coordinate to a row index.
    /// Row 0 = ".." (parent), row 1.. = entries[0..]
    fn y_to_row(&self, y: f32) -> Option<usize> {
        let entries_start = LIST_PADDING + HEADER_HEIGHT + SEP_HEIGHT;
        if y < entries_start {
            return None;
        }
        let offset = y - entries_start;
        let row = (offset / ROW_HEIGHT) as usize;
        let entry_count = self.state.borrow().entries.len();
        if row <= entry_count {
            Some(row)
        } else {
            None
        }
    }
}

impl Widget for FileListWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.scroller.layout(constraints)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        self.scroller.paint(canvas, rect, stride);
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            // Left-click (BTN_LEFT = 0x110 in evdev)
            WidgetEvent::PointerPress { x, y, button: 0x110 } => {
                // Adjust for scroll offset to get content-space Y
                let content_y = *y + self.scroller.scroll_offset;
                if let Some(row) = self.y_to_row(content_y) {
                    let mut state = self.state.borrow_mut();
                    if row == 0 {
                        state.go_parent();
                    } else {
                        let idx = row - 1;
                        if idx < state.entries.len() && state.entries[idx].is_dir {
                            let new_path =
                                state.current_path.join(&state.entries[idx].name);
                            state.navigate(new_path);
                        } else {
                            drop(state);
                            return EventResponse::Ignored;
                        }
                    }
                    drop(state);
                    self.rebuild();
                    return EventResponse::Handled;
                }
                let _ = x; // suppress unused warning
                EventResponse::Ignored
            }

            // Keyboard events (press only)
            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                // Ctrl+H → toggle hidden files
                if ke.modifiers.ctrl && ke.keysym == Keysym::h {
                    self.state.borrow_mut().toggle_hidden();
                    self.rebuild();
                    return EventResponse::Handled;
                }
                // Backspace → parent directory
                if ke.keysym == Keysym::BackSpace {
                    self.state.borrow_mut().go_parent();
                    self.rebuild();
                    return EventResponse::Handled;
                }
                // Delegate scroll keys to scroller
                self.scroller.event(event)
            }

            _ => self.scroller.event(event),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn dirty(&self) -> bool {
        true
    }
}

// ── Main ──

fn main() {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));

    let engine = Rc::new(RefCell::new(TextEngine::new()));
    let state = Rc::new(RefCell::new(FileViewState::new(path.clone(), engine)));

    let root: Box<dyn Widget> = Box::new(FileListWidget::new(Rc::clone(&state)));

    let title = format!("Hayate — {}", path.display());
    if let Err(e) = App::new(title, 900, 700).run(root) {
        eprintln!("Error: {e}");
    }
}
