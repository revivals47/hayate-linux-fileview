#![allow(dead_code)] // Module is under construction; used by terminal_widget (E-5 Step 3).
//! Terminal emulator state: cell grid + ANSI/VT100 parser.
//!
//! Uses the `vte` crate for escape sequence parsing and maintains a 2D grid
//! of character cells with colour attributes. Supports the minimal CSI subset
//! required for bash, ls --color, grep --color, and similar tools.

/// Basic 16-colour palette (SGR 30–37 / 40–47 / 90–97 / 100–107).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color16 {
    Default,
    Black, Red, Green, Yellow, Blue, Magenta, Cyan, White,
    BrightBlack, BrightRed, BrightGreen, BrightYellow,
    BrightBlue, BrightMagenta, BrightCyan, BrightWhite,
}

impl Color16 {
    pub fn to_rgb(self) -> (u8, u8, u8) {
        match self {
            Color16::Default      => (204, 204, 204),
            Color16::Black        => ( 40,  40,  40),
            Color16::Red          => (204,  51,  51),
            Color16::Green        => ( 78, 154,  10),
            Color16::Yellow       => (196, 160,   0),
            Color16::Blue         => ( 52, 101, 164),
            Color16::Magenta      => (117,  80, 123),
            Color16::Cyan         => (  6, 152, 154),
            Color16::White        => (211, 215, 207),
            Color16::BrightBlack  => ( 85,  87,  83),
            Color16::BrightRed    => (239,  41,  41),
            Color16::BrightGreen  => (138, 226,  52),
            Color16::BrightYellow => (252, 233,  79),
            Color16::BrightBlue   => (114, 159, 207),
            Color16::BrightMagenta=> (173, 127, 168),
            Color16::BrightCyan   => ( 52, 226, 226),
            Color16::BrightWhite  => (238, 238, 236),
        }
    }

    fn from_sgr_fg(n: u16) -> Option<Self> {
        match n {
            30 => Some(Self::Black),   31 => Some(Self::Red),
            32 => Some(Self::Green),   33 => Some(Self::Yellow),
            34 => Some(Self::Blue),    35 => Some(Self::Magenta),
            36 => Some(Self::Cyan),    37 => Some(Self::White),
            39 => Some(Self::Default),
            90 => Some(Self::BrightBlack),   91 => Some(Self::BrightRed),
            92 => Some(Self::BrightGreen),   93 => Some(Self::BrightYellow),
            94 => Some(Self::BrightBlue),    95 => Some(Self::BrightMagenta),
            96 => Some(Self::BrightCyan),    97 => Some(Self::BrightWhite),
            _ => None,
        }
    }

    fn from_sgr_bg(n: u16) -> Option<Self> {
        match n {
            40 => Some(Self::Black),   41 => Some(Self::Red),
            42 => Some(Self::Green),   43 => Some(Self::Yellow),
            44 => Some(Self::Blue),    45 => Some(Self::Magenta),
            46 => Some(Self::Cyan),    47 => Some(Self::White),
            49 => Some(Self::Default),
            100 => Some(Self::BrightBlack),  101 => Some(Self::BrightRed),
            102 => Some(Self::BrightGreen),  103 => Some(Self::BrightYellow),
            104 => Some(Self::BrightBlue),   105 => Some(Self::BrightMagenta),
            106 => Some(Self::BrightCyan),   107 => Some(Self::BrightWhite),
            _ => None,
        }
    }
}

// ── Cell ───────────────────────────────────────────────────────────

/// A single character cell in the terminal grid.
#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: Color16,
    pub bg: Color16,
    pub bold: bool,
}

impl Cell {
    fn blank() -> Self {
        Self { ch: ' ', fg: Color16::Default, bg: Color16::Default, bold: false }
    }
}

// ── TerminalState ──────────────────────────────────────────────────

/// Terminal emulator state backed by a fixed-size cell grid.
pub struct TerminalState {
    grid: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    cols: usize,
    rows: usize,
    current_fg: Color16,
    current_bg: Color16,
    current_bold: bool,
    parser: vte::Parser,
}

impl TerminalState {
    pub fn new(cols: usize, rows: usize) -> Self {
        let row = vec![Cell::blank(); cols];
        Self {
            grid: vec![row; rows],
            cursor_row: 0,
            cursor_col: 0,
            cols,
            rows,
            current_fg: Color16::Default,
            current_bg: Color16::Default,
            current_bold: false,
            parser: vte::Parser::new(),
        }
    }

    /// Feed raw bytes (may contain ANSI escapes) through the parser.
    pub fn feed(&mut self, data: &[u8]) {
        // Take the parser out to avoid borrow conflict (parser borrows &mut self).
        let mut parser = std::mem::replace(&mut self.parser, vte::Parser::new());
        parser.advance(self, data);
        self.parser = parser;
    }

    /// Resize the grid. Existing content is preserved (truncated or padded).
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize_with(rows, || vec![Cell::blank(); cols]);
        for row in &mut self.grid {
            row.resize(cols, Cell::blank());
        }
        self.cols = cols;
        self.rows = rows;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.grid[row][col]
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn grid_size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    // ── Internal helpers ──

    fn put_char(&mut self, c: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.linefeed();
        }
        self.grid[self.cursor_row][self.cursor_col] = Cell {
            ch: c,
            fg: self.current_fg,
            bg: self.current_bg,
            bold: self.current_bold,
        };
        self.cursor_col += 1;
    }

    fn linefeed(&mut self) {
        if self.cursor_row + 1 < self.rows {
            self.cursor_row += 1;
        } else {
            // Scroll up: remove first row, push blank at bottom
            self.grid.remove(0);
            self.grid.push(vec![Cell::blank(); self.cols]);
        }
    }

    fn clear_row(&mut self, row: usize, from: usize, to: usize) {
        let end = to.min(self.cols);
        for col in from..end {
            self.grid[row][col] = Cell::blank();
        }
    }

    fn sgr(&mut self, params: &vte::Params) {
        let mut iter = params.iter();
        loop {
            let sub = match iter.next() {
                Some(s) => s,
                None => break,
            };
            let n = sub[0];
            match n {
                0 => {
                    self.current_fg = Color16::Default;
                    self.current_bg = Color16::Default;
                    self.current_bold = false;
                }
                1 => self.current_bold = true,
                22 => self.current_bold = false,
                _ => {
                    if let Some(c) = Color16::from_sgr_fg(n) {
                        self.current_fg = c;
                    } else if let Some(c) = Color16::from_sgr_bg(n) {
                        self.current_bg = c;
                    }
                    // Skip 256-color (38;5;N) and truecolor (38;2;R;G;B) for MVP
                }
            }
        }
    }
}

// ── vte::Perform ───────────────────────────────────────────────────

impl vte::Perform for TerminalState {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => { // BS — backspace
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            0x09 => { // TAB — advance to next 8-column stop
                let next = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next.min(self.cols.saturating_sub(1));
            }
            0x0A | 0x0B | 0x0C => { // LF / VT / FF
                self.linefeed();
            }
            0x0D => { // CR
                self.cursor_col = 0;
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let p1 = params.iter().next().map(|s| s[0]).unwrap_or(0);
        match action {
            // CUU — cursor up
            'A' => {
                let n = (p1 as usize).max(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            // CUD — cursor down
            'B' => {
                let n = (p1 as usize).max(1);
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
            }
            // CUF — cursor forward (right)
            'C' => {
                let n = (p1 as usize).max(1);
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
            }
            // CUB — cursor backward (left)
            'D' => {
                let n = (p1 as usize).max(1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            // CUP — cursor position (1-based)
            'H' | 'f' => {
                let mut pi = params.iter();
                let row = pi.next().map(|s| s[0] as usize).unwrap_or(1).max(1) - 1;
                let col = pi.next().map(|s| s[0] as usize).unwrap_or(1).max(1) - 1;
                self.cursor_row = row.min(self.rows - 1);
                self.cursor_col = col.min(self.cols - 1);
            }
            // ED — erase in display
            'J' => match p1 {
                0 => { // cursor to end
                    self.clear_row(self.cursor_row, self.cursor_col, self.cols);
                    for r in (self.cursor_row + 1)..self.rows {
                        self.clear_row(r, 0, self.cols);
                    }
                }
                1 => { // start to cursor
                    for r in 0..self.cursor_row {
                        self.clear_row(r, 0, self.cols);
                    }
                    self.clear_row(self.cursor_row, 0, self.cursor_col + 1);
                }
                2 | 3 => { // entire screen
                    for r in 0..self.rows {
                        self.clear_row(r, 0, self.cols);
                    }
                }
                _ => {}
            }
            // EL — erase in line
            'K' => match p1 {
                0 => self.clear_row(self.cursor_row, self.cursor_col, self.cols),
                1 => self.clear_row(self.cursor_row, 0, self.cursor_col + 1),
                2 => self.clear_row(self.cursor_row, 0, self.cols),
                _ => {}
            }
            // SGR — select graphic rendition
            'm' => self.sgr(params),
            _ => {}
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_hello() {
        let mut ts = TerminalState::new(80, 24);
        ts.feed(b"hello");
        assert_eq!(ts.cell(0, 0).ch, 'h');
        assert_eq!(ts.cell(0, 1).ch, 'e');
        assert_eq!(ts.cell(0, 2).ch, 'l');
        assert_eq!(ts.cell(0, 3).ch, 'l');
        assert_eq!(ts.cell(0, 4).ch, 'o');
        assert_eq!(ts.cursor(), (0, 5));
    }

    #[test]
    fn feed_newline() {
        let mut ts = TerminalState::new(80, 24);
        ts.feed(b"a\r\nb");
        assert_eq!(ts.cell(0, 0).ch, 'a');
        assert_eq!(ts.cell(1, 0).ch, 'b');
        assert_eq!(ts.cursor(), (1, 1));
    }

    #[test]
    fn feed_csi_cursor_move() {
        let mut ts = TerminalState::new(80, 24);
        // Move to row 3, col 5 (1-based): ESC [ 3 ; 5 H
        ts.feed(b"\x1b[3;5H");
        assert_eq!(ts.cursor(), (2, 4)); // 0-based
        // CUU 1 (up)
        ts.feed(b"\x1b[A");
        assert_eq!(ts.cursor(), (1, 4));
        // CUF 3 (right)
        ts.feed(b"\x1b[3C");
        assert_eq!(ts.cursor(), (1, 7));
    }

    #[test]
    fn feed_sgr_color() {
        let mut ts = TerminalState::new(80, 24);
        // ESC [ 1 ; 31 m (bold + red fg)
        ts.feed(b"\x1b[1;31mX");
        let c = ts.cell(0, 0);
        assert_eq!(c.ch, 'X');
        assert_eq!(c.fg, Color16::Red);
        assert!(c.bold);
        // ESC [ 0 m (reset)
        ts.feed(b"\x1b[0mY");
        let c2 = ts.cell(0, 1);
        assert_eq!(c2.fg, Color16::Default);
        assert!(!c2.bold);
    }

    #[test]
    fn feed_clear_screen() {
        let mut ts = TerminalState::new(80, 24);
        ts.feed(b"ABCDE");
        assert_eq!(ts.cell(0, 0).ch, 'A');
        // ESC [ 2 J — clear entire screen
        ts.feed(b"\x1b[2J");
        assert_eq!(ts.cell(0, 0).ch, ' ');
        assert_eq!(ts.cell(0, 4).ch, ' ');
    }

    #[test]
    fn scroll_on_lf_at_bottom() {
        let mut ts = TerminalState::new(80, 3);
        ts.feed(b"AAA\r\nBBB\r\nCCC\r\nDDD");
        // After 4th line on a 3-row grid: first row scrolled out
        assert_eq!(ts.cell(0, 0).ch, 'B');
        assert_eq!(ts.cell(1, 0).ch, 'C');
        assert_eq!(ts.cell(2, 0).ch, 'D');
    }

    #[test]
    fn tab_stop() {
        let mut ts = TerminalState::new(80, 24);
        ts.feed(b"A\tB");
        assert_eq!(ts.cell(0, 0).ch, 'A');
        // TAB from col 1 → col 8
        assert_eq!(ts.cell(0, 8).ch, 'B');
    }

    #[test]
    fn wrap_at_eol() {
        let mut ts = TerminalState::new(5, 3);
        ts.feed(b"ABCDEFG");
        // 5-col grid: "ABCDE" on row 0, "FG" on row 1
        assert_eq!(ts.cell(0, 4).ch, 'E');
        assert_eq!(ts.cell(1, 0).ch, 'F');
        assert_eq!(ts.cell(1, 1).ch, 'G');
    }
}
