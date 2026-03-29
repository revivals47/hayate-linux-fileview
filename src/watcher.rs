//! Filesystem watcher using inotify(7) on a background thread.
//!
//! Monitors the current directory for file changes (create, delete, modify,
//! rename) and sets an AtomicBool flag that the main loop polls each frame.
//! Events are debounced by 100ms to batch rapid changes.

use std::ffi::CString;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Watches a directory for changes on a background thread.
pub(crate) struct FsWatcher {
    needs_refresh: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    current_path: PathBuf,
}

impl FsWatcher {
    /// Start watching `path` for filesystem events.
    pub(crate) fn new(path: &Path) -> Self {
        let needs_refresh = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread = spawn_watcher(path.to_path_buf(), Arc::clone(&needs_refresh), Arc::clone(&stop_flag));
        Self {
            needs_refresh,
            stop_flag,
            thread: Some(thread),
            current_path: path.to_path_buf(),
        }
    }

    /// Returns true (once) if the watched directory has changed since last check.
    pub(crate) fn needs_refresh(&self) -> bool {
        self.needs_refresh.swap(false, Ordering::Relaxed)
    }

    /// Switch to watching a new directory.
    pub(crate) fn watch(&mut self, new_path: &Path) {
        if self.current_path == new_path {
            return;
        }
        // Stop the old thread
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
        // Start a new watcher
        self.current_path = new_path.to_path_buf();
        self.stop_flag = Arc::new(AtomicBool::new(false));
        self.needs_refresh = Arc::new(AtomicBool::new(false));
        self.thread = Some(spawn_watcher(
            new_path.to_path_buf(),
            Arc::clone(&self.needs_refresh),
            Arc::clone(&self.stop_flag),
        ));
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

// ── inotify background thread ──────────────────────────────────────

const WATCH_MASK: u32 = (libc::IN_CREATE | libc::IN_DELETE | libc::IN_MODIFY
    | libc::IN_MOVED_FROM | libc::IN_MOVED_TO) as u32;

fn spawn_watcher(
    path: PathBuf,
    needs_refresh: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || watcher_thread(path, needs_refresh, stop))
}

fn watcher_thread(path: PathBuf, needs_refresh: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK) };
    if fd < 0 {
        eprintln!("[watcher] inotify_init1 failed");
        return;
    }

    let wd = add_watch(fd, &path);
    if wd < 0 {
        eprintln!("[watcher] inotify_add_watch failed: {}", path.display());
        unsafe { libc::close(fd); }
        return;
    }

    let mut buf = [0u8; 4096];

    while !stop.load(Ordering::Relaxed) {
        // poll() with 100ms timeout — avoids busy-wait
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ret = unsafe { libc::poll(&mut pfd, 1, 100) };

        if ret <= 0 {
            continue; // timeout or error → check stop flag and retry
        }

        // Drain all pending events
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            needs_refresh.store(true, Ordering::Relaxed);
            // Debounce: sleep 100ms to batch rapid successive changes
            thread::sleep(Duration::from_millis(100));
            // Drain any events that arrived during the debounce period
            loop {
                let n2 = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n2 <= 0 { break; }
            }
        }
    }

    unsafe {
        libc::inotify_rm_watch(fd, wd);
        libc::close(fd);
    }
}

fn add_watch(fd: RawFd, path: &Path) -> i32 {
    let Ok(cpath) = CString::new(path.as_os_str().as_encoded_bytes()) else {
        return -1;
    };
    unsafe { libc::inotify_add_watch(fd, cpath.as_ptr(), WATCH_MASK) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn watcher_detects_file_creation() {
        let dir = tempfile::tempdir().unwrap();
        let w = FsWatcher::new(dir.path());

        // No changes yet
        thread::sleep(Duration::from_millis(50));
        // Create a file
        fs::write(dir.path().join("test.txt"), "hello").unwrap();
        // Wait for inotify + debounce
        thread::sleep(Duration::from_millis(250));

        assert!(w.needs_refresh(), "watcher should detect file creation");
        // Second call should return false (consumed)
        assert!(!w.needs_refresh(), "needs_refresh should be consumed");
    }

    #[test]
    fn watcher_switch_directory() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        let mut w = FsWatcher::new(dir1.path());

        w.watch(dir2.path());
        // Allow watcher thread to start and register the inotify watch
        thread::sleep(Duration::from_millis(50));

        // Change in dir2 should be detected
        fs::write(dir2.path().join("file.txt"), "data").unwrap();
        thread::sleep(Duration::from_millis(300));
        assert!(w.needs_refresh());

        // Change in dir1 should NOT be detected
        w.needs_refresh(); // clear
        fs::write(dir1.path().join("old.txt"), "stale").unwrap();
        thread::sleep(Duration::from_millis(250));
        assert!(!w.needs_refresh());
    }
}
