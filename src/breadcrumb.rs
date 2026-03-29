//! Breadcrumb navigation bar showing the current path as clickable segments.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use tiny_skia::Color;
use hayate_ui::platform::keyboard::KeyState;
use hayate_ui::render::{FontFamily, Renderer, TextEngine, TextParams};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::state::FileViewState;

pub(crate) const BAR_HEIGHT: f32 = 24.0;
const FONT_SIZE: f32 = 13.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 4.0;
const BG: (u8, u8, u8) = (35, 35, 40);
const CHAR_W: f32 = 7.5;

const NAV_BTN_W: f32 = CHAR_W * 2.0; // width of ◀ or ▶ button

struct Seg { label: String, path: PathBuf, x0: f32, x1: f32 }

pub(crate) struct BreadcrumbWidget {
    state: Rc<RefCell<FileViewState>>,
    engine: Rc<RefCell<TextEngine>>,
    segs: Vec<Seg>,
    width: f32,
    back_x0: f32, back_x1: f32,
    fwd_x0: f32,  fwd_x1: f32,
    /// Active address bar text input (Ctrl+L).
    editing: Option<hayate_ui::widget::TextInputWidget>,
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
        let mut w = Self {
            state, engine, segs: Vec::new(), width: 0.0,
            back_x0: 0.0, back_x1: 0.0, fwd_x0: 0.0, fwd_x1: 0.0,
            editing: None,
        };
        w.update_segments();
        w
    }

    pub(crate) fn update_segments(&mut self) {
        let cur = self.state.borrow().current_path.clone();
        self.segs.clear();
        let mut x = PAD_X;
        // Back/forward buttons: "◀ ▶ "
        self.back_x0 = x; self.back_x1 = x + NAV_BTN_W;
        x += NAV_BTN_W + CHAR_W; // gap
        self.fwd_x0 = x; self.fwd_x1 = x + NAV_BTN_W;
        x += NAV_BTN_W + CHAR_W; // gap
        // Root "/"
        let mut acc = PathBuf::new();
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

    /// Enter address bar edit mode (Ctrl+L).
    pub(crate) fn start_editing(&mut self) {
        use hayate_ui::widget::TextInputWidget;
        let path_str = self.state.borrow().current_path.display().to_string();
        let mut input = TextInputWidget::new(self.engine.clone()).with_width(self.width);
        input.input_mut().insert_str(&path_str);
        input.input_mut().select_all();
        input.input_mut().set_focused(true);
        self.editing = Some(input);
    }

    /// Commit the typed path and navigate. Returns true if navigation succeeded.
    pub(crate) fn finish_editing(&mut self) -> bool {
        let input = match self.editing.take() {
            Some(w) => w,
            None => return false,
        };
        let text = input.text().trim().to_string();
        let path = PathBuf::from(&text);
        if path.is_dir() {
            self.state.borrow_mut().navigate(path);
            self.update_segments();
            true
        } else {
            eprintln!("[breadcrumb] Invalid path: {text}");
            false
        }
    }

    /// Cancel address bar editing (Escape).
    pub(crate) fn cancel_editing(&mut self) {
        self.editing = None;
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

    fn paint(&self, renderer: &mut Renderer, rect: ItemRect) {
        if let Some((canvas, stride)) = renderer.pixels_mut() {
            let bg_rect = ItemRect::new(rect.x, rect.y, rect.width, BAR_HEIGHT);
            fill_bg(canvas, &bg_rect, stride, BG.0, BG.1, BG.2);

            // Address bar edit mode — draw TextInputWidget instead of breadcrumbs
            if self.editing.is_some() {
                // Fall through to widget paint below
            } else {
                let mut engine = self.engine.borrow_mut();
                let mw = (self.width - PAD_X).max(0.0);
                let st = self.state.borrow();
                // Back/forward buttons
                let back_c = if st.can_go_back() { col(180,180,190) } else { col(60,60,60) };
                let fwd_c = if st.can_go_forward() { col(180,180,190) } else { col(60,60,60) };
                drop(st);
                Self::draw(&mut engine, canvas, stride, &rect, self.back_x0, "◀", back_c, mw);
                Self::draw(&mut engine, canvas, stride, &rect, self.fwd_x0, "▶", fwd_c, mw);
                // Path segments
                let last = self.segs.len().saturating_sub(1);
                for (i, s) in self.segs.iter().enumerate() {
                    if i > 0 {
                        Self::draw(&mut engine, canvas, stride, &rect, s.x0 - CHAR_W*3.0, " > ", col(80,80,90), mw);
                    }
                    let c = if i == last { col(140,200,255) } else { col(160,160,170) };
                    Self::draw(&mut engine, canvas, stride, &rect, s.x0, &s.label, c, mw);
                }
                return;
            }
        }

        // Address bar edit mode — paint TextInputWidget via Renderer
        if let Some(ref input) = self.editing {
            let input_rect = ItemRect::new(rect.x + PAD_X, rect.y, self.width - PAD_X * 2.0, BAR_HEIGHT);
            input.paint(renderer, input_rect);
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        // Address bar edit mode — route events to TextInputWidget
        if self.editing.is_some() {
            // Intercept Escape and Enter before forwarding
            if let WidgetEvent::Key(ke) = event {
                if ke.state == KeyState::Pressed {
                    if ke.keysym == xkbcommon::xkb::Keysym::Escape {
                        self.cancel_editing();
                        return EventResponse::Handled;
                    }
                    if ke.keysym == xkbcommon::xkb::Keysym::Return
                        || ke.keysym == xkbcommon::xkb::Keysym::KP_Enter
                    {
                        self.finish_editing();
                        return EventResponse::Handled;
                    }
                }
            }
            if let Some(ref mut input) = self.editing {
                input.event(event);
            }
            return EventResponse::Handled;
        }

        match event {
            WidgetEvent::PointerPress { x, button: 0x110, .. } => {
                // Back button
                if *x >= self.back_x0 && *x < self.back_x1 {
                    self.state.borrow_mut().go_back();
                    self.update_segments();
                    return EventResponse::Handled;
                }
                // Forward button
                if *x >= self.fwd_x0 && *x < self.fwd_x1 {
                    self.state.borrow_mut().go_forward();
                    self.update_segments();
                    return EventResponse::Handled;
                }
                // Path segment
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
                    self.start_editing();
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            }
            _ => EventResponse::Ignored,
        }
    }

    fn dirty(&self) -> bool { true }
}
