#![allow(dead_code)] // Module is under construction; integrated in E-5 Step 4.
//! Terminal emulator widget: bridges PTY ↔ TerminalState ↔ Widget rendering.
//!
//! Spawns a shell via [`Pty`], feeds output into [`TerminalState`], and paints
//! the cell grid using [`TextEngine`].  Keyboard events are translated to
//! terminal escape sequences and written to the PTY.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use xkbcommon::xkb::Keysym;

use hayate_ui::platform::keyboard::{KeyEvent, KeyState};
use hayate_ui::render::{FontFamily, Renderer, TextEngine, TextParams};
use hayate_ui::scroll::delegate::ItemRect;
use hayate_ui::widget::core::{Constraints, EventResponse, Size, Widget, WidgetEvent};

use crate::terminal_pty::Pty;
use crate::terminal_state::{Color16, TerminalState};

const FONT_SIZE: f32 = 14.0;
const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 24;

/// Terminal emulator widget — PTY + ANSI grid + rendering.
pub(crate) struct TerminalWidget {
    pty: Option<Pty>,
    state: TerminalState,
    engine: Rc<RefCell<TextEngine>>,
    is_dirty: bool,
    cell_width: f32,
    cell_height: f32,
    width: f32,
    height: f32,
}

impl TerminalWidget {
    pub(crate) fn new(engine: Rc<RefCell<TextEngine>>) -> Self {
        let (cw, ch) = measure_cell(&engine);
        Self {
            pty: None,
            state: TerminalState::new(DEFAULT_COLS, DEFAULT_ROWS),
            engine,
            is_dirty: true,
            cell_width: cw,
            cell_height: ch,
            width: 0.0,
            height: 0.0,
        }
    }

    /// Spawn a shell in the given working directory.
    pub(crate) fn spawn(&mut self, cwd: &Path) {
        let (cols, rows) = self.state.grid_size();
        match Pty::spawn(cwd, cols as u16, rows as u16) {
            Ok(pty) => self.pty = Some(pty),
            Err(e) => eprintln!("[terminal] spawn failed: {e}"),
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.pty.is_some()
    }

    /// Poll PTY for new output and feed into the ANSI parser.
    pub(crate) fn poll(&mut self) {
        if let Some(ref pty) = self.pty
            && pty.has_output() {
                let data = pty.take_output();
                self.state.feed(&data);
                self.is_dirty = true;
            }
    }

    /// Send a `cd` command to the shell.
    pub(crate) fn change_dir(&self, path: &Path) {
        if let Some(ref pty) = self.pty {
            pty.change_dir(path);
        }
    }
}

impl Widget for TerminalWidget {
    fn layout(&mut self, constraints: &Constraints) -> Size {
        self.width = constraints.max_width;
        self.height = constraints.max_height;

        // Compute grid dimensions from available space
        let cols = (self.width / self.cell_width).floor().max(1.0) as usize;
        let rows = (self.height / self.cell_height).floor().max(1.0) as usize;
        let (old_cols, old_rows) = self.state.grid_size();
        if cols != old_cols || rows != old_rows {
            self.state.resize(cols, rows);
            if let Some(ref pty) = self.pty {
                pty.resize(cols as u16, rows as u16);
            }
            self.is_dirty = true;
        }

        Size::new(self.width, self.height)
    }

    fn paint(&mut self, renderer: &mut Renderer, rect: ItemRect) {
        if let Some((canvas, stride)) = renderer.pixels_mut() {
            let (cols, rows) = self.state.grid_size();
            let (cursor_row, cursor_col) = self.state.cursor();
            let canvas_h = canvas.len() as u32 / stride;
            let mut engine = self.engine.borrow_mut();

            for row in 0..rows {
                let py = rect.y + row as f32 * self.cell_height;
                if py + self.cell_height < rect.y || py >= rect.y + rect.height { continue; }

                for col in 0..cols {
                    let px = rect.x + col as f32 * self.cell_width;
                    if px + self.cell_width < rect.x || px >= rect.x + rect.width { continue; }

                    let cell = self.state.cell(row, col);
                    let is_cursor = row == cursor_row && col == cursor_col;

                    // Background
                    let (bg_r, bg_g, bg_b) = if is_cursor {
                        (200u8, 200, 200) // cursor block: bright
                    } else if cell.bg != Color16::Default {
                        cell.bg.to_rgb()
                    } else {
                        continue; // transparent (painted by parent)
                    };
                    paint_cell_bg(canvas, stride, canvas_h, px, py,
                        self.cell_width, self.cell_height, bg_r, bg_g, bg_b);
                }

                // Draw text: build row string, then draw entire row at once (much faster)
                let mut row_text = String::with_capacity(cols);
                let mut first_non_space = cols;
                let mut last_non_space = 0;
                for col in 0..cols {
                    let ch = self.state.cell(row, col).ch;
                    row_text.push(ch);
                    if ch != ' ' {
                        if col < first_non_space { first_non_space = col; }
                        last_non_space = col;
                    }
                }

                // Skip entirely blank rows
                if first_non_space >= cols { continue; }

                let fg_cell = self.state.cell(row, first_non_space);
                let (fr, fg, fb) = if fg_cell.bold {
                    brighten(fg_cell.fg.to_rgb())
                } else {
                    fg_cell.fg.to_rgb()
                };
                let trimmed: String = row_text[first_non_space..=last_non_space].to_string();
                let color = tiny_skia::Color::from_rgba8(fr, fg, fb, 255);
                let params = TextParams {
                    text: &trimmed, font_size: FONT_SIZE,
                    line_height: self.cell_height, color,
                    family: FontFamily::Monospace,
                };
                let buf = engine.layout(&params, self.width);
                let draw_x = rect.x + first_non_space as f32 * self.cell_width;
                engine.draw_buffer(&buf, canvas, stride,
                    draw_x as i32, py as i32, color,
                    stride / 4, canvas_h);

                // Cursor text: invert character at cursor position
                if row == cursor_row && cursor_col < cols {
                    let cc = self.state.cell(cursor_row, cursor_col);
                    if cc.ch != ' ' {
                        let s = cc.ch.to_string();
                        let inv_color = tiny_skia::Color::from_rgba8(30, 30, 35, 255);
                        let p = TextParams {
                            text: &s, font_size: FONT_SIZE,
                            line_height: self.cell_height, color: inv_color,
                            family: FontFamily::Monospace,
                        };
                        let b = engine.layout(&p, self.cell_width * 2.0);
                        let cx = rect.x + cursor_col as f32 * self.cell_width;
                        engine.draw_buffer(&b, canvas, stride,
                            cx as i32, py as i32, inv_color,
                            stride / 4, canvas_h);
                    }
                }
            }
        }
    }

    fn event(&mut self, event: &WidgetEvent) -> EventResponse {
        if let WidgetEvent::Key(ke) = event {
            if ke.state != KeyState::Pressed { return EventResponse::Ignored; }
            if let Some(bytes) = key_to_bytes(ke) {
                if let Some(ref pty) = self.pty {
                    let _ = pty.write_input(&bytes);
                }
                return EventResponse::Handled;
            }
        }
        EventResponse::Ignored
    }

    fn focusable(&self) -> bool { true }

    fn dirty(&self) -> bool {
        if self.is_dirty { return true; }
        if let Some(ref pty) = self.pty {
            return pty.has_output();
        }
        false
    }

    fn clear_dirty(&mut self) {
        self.poll(); // consume any pending output
        self.is_dirty = false;
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Measure the cell dimensions for the monospace font at FONT_SIZE.
fn measure_cell(engine: &Rc<RefCell<TextEngine>>) -> (f32, f32) {
    let mut eng = engine.borrow_mut();
    let params = TextParams {
        text: "M", font_size: FONT_SIZE, line_height: FONT_SIZE * 1.3,
        color: tiny_skia::Color::WHITE, family: FontFamily::Monospace,
    };
    let buf = eng.layout(&params, 100.0);
    let mut w = FONT_SIZE * 0.6; // fallback
    for run in buf.layout_runs() {
        for glyph in run.glyphs.iter() {
            w = glyph.w;
        }
    }
    (w.max(1.0), (FONT_SIZE * 1.3).max(1.0))
}

/// Paint a cell background rectangle.
#[allow(clippy::too_many_arguments)]
fn paint_cell_bg(
    canvas: &mut [u8], stride: u32, canvas_h: u32,
    x: f32, y: f32, w: f32, h: f32,
    r: u8, g: u8, b: u8,
) {
    let x0 = x.max(0.0) as u32;
    let y0 = y.max(0.0) as u32;
    let x1 = ((x + w) as u32).min(stride / 4);
    let y1 = ((y + h) as u32).min(canvas_h);
    for py in y0..y1 {
        let row = (py * stride) as usize;
        for px in x0..x1 {
            let off = row + (px * 4) as usize;
            if off + 3 < canvas.len() {
                canvas[off] = b;     // BGRA
                canvas[off + 1] = g;
                canvas[off + 2] = r;
                canvas[off + 3] = 255;
            }
        }
    }
}

/// Slightly brighten a color for bold text.
fn brighten((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    (r.saturating_add(40), g.saturating_add(40), b.saturating_add(40))
}

/// Convert a KeyEvent to bytes for the PTY.
fn key_to_bytes(ke: &KeyEvent) -> Option<Vec<u8>> {
    match ke.keysym {
        Keysym::Return | Keysym::KP_Enter => return Some(b"\r".to_vec()),
        Keysym::BackSpace => return Some(vec![0x7f]),
        Keysym::Tab => return Some(b"\t".to_vec()),
        Keysym::Escape => return Some(vec![0x1b]),
        Keysym::Up => return Some(b"\x1b[A".to_vec()),
        Keysym::Down => return Some(b"\x1b[B".to_vec()),
        Keysym::Right => return Some(b"\x1b[C".to_vec()),
        Keysym::Left => return Some(b"\x1b[D".to_vec()),
        Keysym::Home => return Some(b"\x1b[H".to_vec()),
        Keysym::End => return Some(b"\x1b[F".to_vec()),
        Keysym::Delete => return Some(b"\x1b[3~".to_vec()),
        Keysym::Page_Up => return Some(b"\x1b[5~".to_vec()),
        Keysym::Page_Down => return Some(b"\x1b[6~".to_vec()),
        _ => {}
    }
    // Ctrl+letter → control character 0x01..0x1a
    if ke.modifiers.ctrl
        && let Some(ref s) = ke.utf8
            && let Some(c) = s.chars().next() {
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_lowercase() {
                    return Some(vec![lower as u8 - b'a' + 1]);
                }
            }
    // Regular printable character
    ke.utf8.as_ref().map(|s| s.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_to_bytes_enter() {
        let ke = KeyEvent {
            key: 0, keysym: Keysym::Return,
            utf8: None, modifiers: Default::default(), state: KeyState::Pressed,
        };
        assert_eq!(key_to_bytes(&ke), Some(b"\r".to_vec()));
    }

    #[test]
    fn key_to_bytes_arrow() {
        let ke = KeyEvent {
            key: 0, keysym: Keysym::Up,
            utf8: None, modifiers: Default::default(), state: KeyState::Pressed,
        };
        assert_eq!(key_to_bytes(&ke), Some(b"\x1b[A".to_vec()));
    }

    #[test]
    fn key_to_bytes_ctrl_c() {
        let mut mods = hayate_ui::platform::keyboard::Modifiers::default();
        mods.ctrl = true;
        let ke = KeyEvent {
            key: 0, keysym: Keysym::c,
            utf8: Some("c".into()), modifiers: mods, state: KeyState::Pressed,
        };
        assert_eq!(key_to_bytes(&ke), Some(vec![0x03])); // ETX
    }

    #[test]
    fn key_to_bytes_printable() {
        let ke = KeyEvent {
            key: 0, keysym: Keysym::a,
            utf8: Some("a".into()), modifiers: Default::default(), state: KeyState::Pressed,
        };
        assert_eq!(key_to_bytes(&ke), Some(b"a".to_vec()));
    }

    #[test]
    fn measure_cell_nonzero() {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        let (w, h) = measure_cell(&engine);
        assert!(w > 0.0, "cell width should be > 0");
        assert!(h > 0.0, "cell height should be > 0");
    }
}
