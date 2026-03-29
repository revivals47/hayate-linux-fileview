# E-5: Terminal Integration Design

## Architecture Overview

```
┌─────────────────────────────────────────┐
│              three_pane.rs              │
│  F12 toggle → terminal_height > 0      │
│  layout: 3-pane area shrinks, terminal  │
│  panel appears at bottom               │
├─────────────────────────────────────────┤
│           terminal_widget.rs            │
│  Widget trait impl (paint/event/layout) │
│  Renders cell grid from TerminalState   │
│  Routes keyboard → PTY write            │
├─────────────────────────────────────────┤
│           terminal_state.rs             │
│  Cell grid (rows x cols)                │
│  Cursor position, scroll-back buffer    │
│  ANSI parser (vte crate Perform trait)  │
├─────────────────────────────────────────┤
│             terminal_pty.rs             │
│  PTY master/slave via libc::openpty()   │
│  Spawns /bin/bash child process         │
│  I/O: read thread → AtomicBool dirty    │
│  Write: direct to master FD             │
└─────────────────────────────────────────┘
```

## Module Structure (4 new files)

### 1. terminal_pty.rs (~120 lines)
PTY lifecycle management.

```rust
pub struct Pty {
    master_fd: OwnedFd,
    child_pid: libc::pid_t,
    // Read thread → shared buffer
    output_buf: Arc<Mutex<Vec<u8>>>,
    dirty: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    reader_thread: Option<JoinHandle<()>>,
}

impl Pty {
    pub fn spawn(shell: &str, cwd: &Path, cols: u16, rows: u16) -> io::Result<Self>;
    pub fn write(&self, data: &[u8]) -> io::Result<usize>;
    pub fn take_output(&self) -> Vec<u8>;
    pub fn needs_redraw(&self) -> bool;  // dirty.swap(false)
    pub fn resize(&self, cols: u16, rows: u16);  // TIOCSWINSZ ioctl
    pub fn cd(&self, path: &Path);  // write "cd <path>\n" to PTY
}

impl Drop for Pty {
    // kill child, stop reader thread, close FD
}
```

**PTY creation via libc:**
```rust
fn create_pty(cols: u16, rows: u16) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    let mut ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) };
    // fork, setsid, dup2 slave→stdin/out/err, exec shell
}
```

**I/O strategy: Separate read thread (same pattern as C-3 watcher)**
- Reader thread: poll(master_fd, 16ms) → read → Arc<Mutex<Vec<u8>>> → dirty.store(true)
- Writer: direct libc::write to master_fd from main thread (small payloads, non-blocking)
- Why not calloop Generic: calloop is owned by WaylandWindow::run(), not accessible from widget code. The AtomicBool pattern already proven by FsWatcher is simpler and requires no hayate-ui changes.

### 2. terminal_state.rs (~250 lines)
Terminal grid model + ANSI parsing.

```rust
pub struct TerminalState {
    grid: Vec<Vec<Cell>>,
    cols: u16,
    rows: u16,
    cursor: (u16, u16),  // (row, col)
    // Scroll-back buffer
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_max: usize,
    // Parser state
    parser: vte::Parser,
    // Style state
    fg: Color,
    bg: Color,
    bold: bool,
    // Alternate screen buffer (for vim, less, etc.)
    alt_grid: Option<Vec<Vec<Cell>>>,
    alt_cursor: Option<(u16, u16)>,
}

#[derive(Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl vte::Perform for TerminalState {
    fn print(&mut self, c: char);     // Regular character
    fn execute(&mut self, byte: u8);  // C0 controls (\n, \r, \t, \x08)
    fn csi_dispatch(&mut self, params, intermediates, ignore, action);  // CSI sequences
    fn esc_dispatch(&mut self, ...);  // ESC sequences
    fn osc_dispatch(&mut self, ...);  // OSC (title change, etc.)
}
```

### 3. terminal_widget.rs (~200 lines)
Widget rendering and input routing.

```rust
pub struct TerminalWidget {
    state: TerminalState,
    pty: Pty,
    engine: Rc<RefCell<TextEngine>>,
    cell_width: f32,   // monospace glyph width
    cell_height: f32,  // line height
    width: f32,
    height: f32,
}

impl Widget for TerminalWidget {
    fn layout(&mut self, constraints) -> Size;
    fn paint(&self, canvas, rect, stride);  // render cell grid
    fn event(&mut self, event) -> EventResponse;  // keyboard → pty.write()
}
```

### 4. three_pane.rs changes (~15 lines)
F12 toggle, split layout.

```rust
// New field:
terminal: Option<TerminalWidget>,
terminal_height: f32,  // 0 = hidden, 200 = default

// F12 handling in event():
Keysym::F12 => {
    if self.terminal.is_some() { self.terminal_height = if h > 0 { 0 } else { 200 }; }
    else { self.terminal = Some(TerminalWidget::new(...)); self.terminal_height = 200; }
}

// layout(): content_height -= terminal_height; terminal gets bottom strip
// paint(): terminal painted below the 3-pane area
```

## Dependency Candidates

| Crate | Purpose | Size | Decision |
|-------|---------|------|----------|
| `vte` 0.15 | ANSI/VT parser (state machine) | ~3K LOC | **Use** — battle-tested (Alacritty), zero alloc |
| `libc` 0.2 | openpty/ioctl/fork | Already dep | **Use** |
| `alacritty_terminal` | Full terminal emulator | ~15K LOC | **Skip** — too large, we want minimal |
| `portable-pty` | Cross-platform PTY | ~2K LOC | **Skip** — Linux only is fine, libc::openpty suffices |

**Only 1 new crate: `vte`** — everything else uses libc (already a dependency).

## ANSI Escape Sequence Scope

### MVP (Minimum Viable Product)
Enough for bash prompt, ls, cat, grep:

| Category | Sequences | Complexity |
|----------|-----------|-----------|
| Cursor move | CUU/CUD/CUF/CUB, CUP, CR, LF | Low |
| Erase | ED (clear screen), EL (clear line) | Low |
| SGR colors | 8 basic + bright (0-15), reset | Low |
| Scroll | LF at bottom → scroll up | Low |
| Tab | HT (horizontal tab, 8-col stops) | Low |
| Bell | BEL (ignore or flash) | Trivial |

**Estimated: ~100 lines of CSI dispatch**

### Phase 2 (post-MVP)
Needed for vim, less, htop:

| Category | Sequences |
|----------|-----------|
| Alt screen | DECSET/DECRST 1049 (smcup/rmcup) |
| Scroll regions | DECSTBM (set top/bottom margins) |
| 256-color | SGR 38;5;N and 48;5;N |
| True color | SGR 38;2;R;G;B and 48;2;R;G;B |
| Cursor visibility | DECTCEM show/hide |
| Insert/delete | ICH, DCH, IL, DL |
| Bracketed paste | DECSET 2004 |

**Estimated: +150 lines of CSI dispatch**

### Not planned
- Sixel graphics, OSC hyperlinks, Unicode grapheme clusters beyond basic width

## Integration with fileview

### Auto-cd on directory navigation
```rust
// In state.rs navigate()/go_back()/go_forward():
if let Some(ref pty) = self.terminal_pty {
    pty.cd(&self.current_path);
}
```

### Dirty checking (same pattern as FsWatcher)
```rust
// In three_pane.rs layout():
if let Some(ref term) = self.terminal {
    if term.pty.needs_redraw() {
        let output = term.pty.take_output();
        term.state.process(&output);  // feed through vte parser
    }
}
```

## Complexity Assessment

| Component | Effort | Risk |
|-----------|--------|------|
| PTY spawn + I/O | Low | Low — libc::openpty is well-documented |
| ANSI parser (MVP) | Low | Low — vte crate handles parsing |
| Cell grid rendering | Medium | Low — monospace grid is straightforward |
| Keyboard input mapping | Medium | Medium — special keys (arrows, Ctrl+C, etc.) need encoding |
| Scroll-back buffer | Low | Low — VecDeque ring buffer |
| three_pane integration | Low | Medium — 499 lines, very tight |
| Alt screen (Phase 2) | Medium | Low — swap grid pointer |
| Resize handling | Medium | Medium — TIOCSWINSZ + reflow |

### Key Risks

1. **three_pane.rs at 499 lines**: Terminal toggle must be minimal. Suggest extracting terminal layout logic into terminal_widget.rs entirely, with three_pane only holding `Option<TerminalWidget>` and delegating.

2. **Keyboard encoding complexity**: Arrow keys, function keys, Ctrl+key combos, and mouse reporting require careful xterm-compatible encoding. MVP skips mouse reporting.

3. **Performance**: Large output (e.g., `cat large_file`) floods the PTY buffer. Need a cap on buffered output and frame-rate limiting (already natural from 60fps draw loop).

4. **UTF-8 + wide chars**: CJK characters occupy 2 cells. Unicode-width crate (already a dependency) handles this.

## MVP Implementation Order

1. **terminal_pty.rs** — PTY spawn + read thread + write (testable standalone)
2. **terminal_state.rs** — Cell grid + vte::Perform (testable with canned ANSI input)
3. **terminal_widget.rs** — Widget rendering (visual testing)
4. **three_pane.rs** — F12 toggle + layout split
5. **Keyboard routing** — special key encoding

Estimated total: ~700 lines across 4 files (all under 500 each).
