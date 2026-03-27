//! Breadcrumb navigation bar showing the current path as clickable segments.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use tiny_skia::Color;
use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::render::{FontFamily, TextEngine, TextParams};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::state::FileViewState;

pub(crate) const BAR_HEIGHT: f32 = 24.0;
const FONT_SIZE: f32 = 13.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 4.0;
const BG: (u8, u8, u8) = (35, 35, 40);
const CHAR_W: f32 = 7.5;

struct Seg { label: String, path: PathBuf, x0: f32, x1: f32 }

pub(crate) struct BreadcrumbWidget {
    state: Rc<RefCell<FileViewState>>,
    engine: Rc<RefCell<TextEngine>>,
    segs: Vec<Seg>,
    width: f32,
}

fn col(r: u8, g: u8, b: u8) -> Color { Color::from_rgba8(r, g, b, 255) }

fn fill_bg(canvas: &mut [u8], rect: &ItemRect, stride: u32, r: u8, g: u8, b: u8) {
    let (x0, y0) = (rect.x.max(0.0) as u32, rect.y.max(0.0) as u32);
    let (x1, y1) = ((rect.x + rect.width) as u32, (rect.y + rect.height) as u32);
    let h = canvas.len() as u32 / stride;
    for py in y0..y1.min(h) {
        for px in x0..x1 {
            let o = (py * stride + px * 4) as usize;
            if o + 3 < canvas.len() { canvas[o] = b; canvas[o+1] = g; canvas[o+2] = r; canvas[o+3] = 255; }
        }
    }
}

impl BreadcrumbWidget {
    pub(crate) fn new(state: Rc<RefCell<FileViewState>>, engine: Rc<RefCell<TextEngine>>) -> Self {
        let mut w = Self { state, engine, segs: Vec::new(), width: 0.0 };
        w.update_segments();
        w
    }

    pub(crate) fn update_segments(&mut self) {
        let cur = self.state.borrow().current_path.clone();
        self.segs.clear();
        let mut acc = PathBuf::new();
        let mut x = PAD_X;
        // Root "/"
        acc.push("/");
        let lw = CHAR_W;
        self.segs.push(Seg { label: "/".into(), path: acc.clone(), x0: x, x1: x + lw });
        x += lw;
        for c in cur.components().skip(1) {
            let name = c.as_os_str().to_string_lossy().to_string();
            acc.push(&name);
            x += CHAR_W * 3.0; // " > "
            let lw = CHAR_W * name.len() as f32;
            self.segs.push(Seg { label: name, path: acc.clone(), x0: x, x1: x + lw });
            x += lw;
        }
    }

    fn seg_at(&self, x: f32) -> Option<&Seg> {
        self.segs.iter().find(|s| x >= s.x0 && x < s.x1)
    }

    fn draw(engine: &mut TextEngine, canvas: &mut [u8], stride: u32, rect: &ItemRect,
            x: f32, text: &str, color: Color, w: f32) {
        let p = TextParams { text, font_size: FONT_SIZE, line_height: BAR_HEIGHT, color, family: FontFamily::Monospace };
        let buf = engine.layout(&p, w);
        engine.draw_buffer(&buf, canvas, stride, (rect.x+x) as i32, (rect.y+PAD_Y) as i32, color, w as u32, BAR_HEIGHT as u32);
    }
}

impl Widget for BreadcrumbWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.width = constraints.max_width;
        Size::new(constraints.max_width, BAR_HEIGHT)
    }

    fn paint(&self, canvas: &mut [u8], rect: ItemRect, stride: u32) {
        let bg_rect = ItemRect::new(rect.x, rect.y, rect.width, BAR_HEIGHT);
        fill_bg(canvas, &bg_rect, stride, BG.0, BG.1, BG.2);

        let mut engine = self.engine.borrow_mut();
        let mw = (self.width - PAD_X).max(0.0);
        let last = self.segs.len().saturating_sub(1);
        for (i, s) in self.segs.iter().enumerate() {
            if i > 0 {
                Self::draw(&mut engine, canvas, stride, &rect, s.x0 - CHAR_W*3.0, " > ", col(80,80,90), mw);
            }
            let c = if i == last { col(140,200,255) } else { col(160,160,170) };
            Self::draw(&mut engine, canvas, stride, &rect, s.x0, &s.label, c, mw);
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        match event {
            WidgetEvent::PointerPress { x, button: 0x110, .. } => {
                if let Some(s) = self.seg_at(*x) {
                    let p = s.path.clone();
                    if p.is_dir() {
                        self.state.borrow_mut().navigate(p);
                        self.update_segments();
                        return EventResponse::Handled;
                    }
                }
                EventResponse::Ignored
            }
            WidgetEvent::Key(ke) if ke.state == KeyState::Pressed => {
                if ke.modifiers.ctrl && ke.keysym == xkbcommon::xkb::Keysym::l {
                    eprintln!("[breadcrumb] Ctrl+L: address bar edit mode (stub)");
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn dirty(&self) -> bool { true }
}
