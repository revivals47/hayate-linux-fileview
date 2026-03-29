//! PTY (pseudo-terminal) management: shell spawn, I/O, and resize.
//!
//! Uses libc syscalls directly (openpty, fork, execvp) — no extra crates.
//! Output is read on a background thread and buffered for the main loop
//! to consume via [`Pty::take_output`].  Same AtomicBool pattern as FsWatcher.

use std::ffi::CString;
use std::io;
use std::os::fd::RawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// A pseudo-terminal connected to a shell process.
// Will be used by terminal_widget (E-5 Step 3).
#[allow(dead_code)]
pub(crate) struct Pty {
    master_fd: RawFd,
    child_pid: libc::pid_t,
    reader_thread: Option<thread::JoinHandle<()>>,
    output_buffer: Arc<Mutex<Vec<u8>>>,
    has_output: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl Pty {
    /// Open a PTY and spawn a shell process.
    ///
    /// `cols` and `rows` set the initial terminal size.  The shell's working
    /// directory is set to `cwd`.
    pub(crate) fn spawn(cwd: &Path, cols: u16, rows: u16) -> io::Result<Self> {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;

        let ret = unsafe {
            libc::openpty(
                &mut master, &mut slave,
                std::ptr::null_mut(), std::ptr::null(), &ws,
            )
        };
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }

        let child_pid = unsafe { libc::fork() };
        if child_pid < 0 {
            unsafe { libc::close(master); libc::close(slave); }
            return Err(io::Error::last_os_error());
        }

        if child_pid == 0 {
            // ── Child process ──
            unsafe {
                libc::close(master);
                libc::setsid();
                libc::dup2(slave, libc::STDIN_FILENO);
                libc::dup2(slave, libc::STDOUT_FILENO);
                libc::dup2(slave, libc::STDERR_FILENO);
                if slave > 2 { libc::close(slave); }

                // Set working directory
                if let Ok(cpath) = CString::new(cwd.as_os_str().as_encoded_bytes()) {
                    libc::chdir(cpath.as_ptr());
                }

                // Set TERM for color support
                let term = CString::new("TERM=xterm-256color").unwrap();
                libc::putenv(term.as_ptr() as *mut _);

                // exec shell
                let shell = CString::new(
                    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
                ).unwrap();
                let args = [shell.as_ptr(), std::ptr::null()];
                libc::execvp(shell.as_ptr(), args.as_ptr());
                libc::_exit(127);
            }
        }

        // ── Parent process ──
        unsafe { libc::close(slave); }

        // Set master to non-blocking for the reader thread's poll loop
        unsafe {
            let flags = libc::fcntl(master, libc::F_GETFL);
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        let output_buffer = Arc::new(Mutex::new(Vec::with_capacity(4096)));
        let has_output = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));

        let reader = spawn_reader(
            master,
            Arc::clone(&output_buffer),
            Arc::clone(&has_output),
            Arc::clone(&stop_flag),
        );

        Ok(Self {
            master_fd: master,
            child_pid,
            reader_thread: Some(reader),
            output_buffer,
            has_output,
            stop_flag,
        })
    }

    /// Check if new output is available (consume-once).
    pub(crate) fn has_output(&self) -> bool {
        self.has_output.swap(false, Ordering::Relaxed)
    }

    /// Take all buffered output bytes.
    pub(crate) fn take_output(&self) -> Vec<u8> {
        let mut buf = self.output_buffer.lock().unwrap();
        std::mem::take(&mut *buf)
    }

    /// Write input bytes to the shell (keyboard data, escape sequences).
    pub(crate) fn write_input(&self, data: &[u8]) -> io::Result<()> {
        let ret = unsafe {
            libc::write(self.master_fd, data.as_ptr() as *const libc::c_void, data.len())
        };
        if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// Notify the shell of a terminal size change.
    pub(crate) fn resize(&self, cols: u16, rows: u16) {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        unsafe { libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws); }
    }

    /// Send a `cd <path>` command to the shell.
    pub(crate) fn change_dir(&self, path: &Path) {
        // Use printf-style quoting: single-quote the path, escaping existing quotes
        let escaped = path.display().to_string().replace('\'', "'\\''");
        let cmd = format!("cd '{}'\n", escaped);
        let _ = self.write_input(cmd.as_bytes());
    }

    /// The child shell's PID.
    #[cfg(test)]
    pub(crate) fn child_pid(&self) -> libc::pid_t {
        self.child_pid
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.reader_thread.take() {
            t.join().ok();
        }
        unsafe {
            libc::close(self.master_fd);
            libc::kill(self.child_pid, libc::SIGTERM);
            // Block-wait to reap the child (no zombies)
            libc::waitpid(self.child_pid, std::ptr::null_mut(), 0);
        }
    }
}

// ── Reader thread ──────────────────────────────────────────────────

#[allow(dead_code)]
fn spawn_reader(
    master_fd: RawFd,
    buffer: Arc<Mutex<Vec<u8>>>,
    has_output: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut read_buf = [0u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            // poll with 50ms timeout
            let mut pfd = libc::pollfd { fd: master_fd, events: libc::POLLIN, revents: 0 };
            let ret = unsafe { libc::poll(&mut pfd, 1, 50) };
            if ret <= 0 { continue; }

            loop {
                let n = unsafe {
                    libc::read(master_fd, read_buf.as_mut_ptr() as *mut libc::c_void, read_buf.len())
                };
                if n <= 0 { break; }
                let mut buf = buffer.lock().unwrap();
                // Cap buffer at 256KB to prevent unbounded growth
                if buf.len() < 256 * 1024 {
                    buf.extend_from_slice(&read_buf[..n as usize]);
                }
                has_output.store(true, Ordering::Relaxed);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn spawn_and_write() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(dir.path(), 80, 24).unwrap();
        assert!(pty.child_pid() > 0);

        // Wait for shell to start and produce prompt
        thread::sleep(Duration::from_millis(300));
        assert!(pty.has_output(), "shell should produce initial output");
        let output = pty.take_output();
        assert!(!output.is_empty());
    }

    #[test]
    fn echo_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(dir.path(), 80, 24).unwrap();
        thread::sleep(Duration::from_millis(200));
        pty.take_output(); // drain prompt

        // Send a command and check output
        pty.write_input(b"echo HAYATE_TEST_MARKER\n").unwrap();
        thread::sleep(Duration::from_millis(300));
        let output = pty.take_output();
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("HAYATE_TEST_MARKER"), "expected marker in: {text}");
    }

    #[test]
    fn resize_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(dir.path(), 80, 24).unwrap();
        pty.resize(120, 40);
        // No crash = success
    }

    #[test]
    fn change_dir_writes_command() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(dir.path(), 80, 24).unwrap();
        thread::sleep(Duration::from_millis(200));
        pty.take_output(); // drain

        pty.change_dir(Path::new("/tmp"));
        thread::sleep(Duration::from_millis(300));
        let output = pty.take_output();
        let text = String::from_utf8_lossy(&output);
        // The cd command should appear in terminal output (echo or prompt change)
        assert!(text.contains("cd") || text.contains("/tmp"), "output: {text}");
    }

    #[test]
    fn drop_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(dir.path(), 80, 24).unwrap();
        let pid = pty.child_pid();
        drop(pty);
        // Give OS time to reap
        thread::sleep(Duration::from_millis(100));
        // Process should be gone (kill returns error for non-existent pid)
        let ret = unsafe { libc::kill(pid, 0) };
        assert!(ret != 0, "child process should be terminated");
    }
}
