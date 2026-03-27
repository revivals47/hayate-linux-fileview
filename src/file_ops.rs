//! File operation utilities for drag-and-drop and clipboard integration.
//!
//! This module provides the logic layer for file copy/move/delete operations.
//! Actual DnD event handling will be integrated once hayate-ui exposes DnD events.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Result of a file operation.
#[derive(Debug)]
pub enum FileOpResult {
    /// Number of files successfully processed.
    Done(usize),
    /// Partial success: completed count and first error encountered.
    Partial(usize, io::Error),
}

/// Parse a `text/uri-list` payload (as defined by RFC 2483) into file paths.
///
/// - Lines starting with `#` are comments and skipped.
/// - Each URI is expected to be `file:///...`; non-file URIs are ignored.
/// - Percent-encoded bytes (`%XX`) are decoded.
pub fn parse_uri_list(data: &str) -> Vec<PathBuf> {
    data.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|uri| {
            let stripped = uri.strip_prefix("file://")?;
            Some(PathBuf::from(percent_decode(stripped)))
        })
        .collect()
}

/// Decode percent-encoded bytes in a URI path component.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(
                &input[i + 1..i + 3],
                16,
            ) {
                out.push(val);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Copy a single file or directory (recursively) into `dest_dir`.
///
/// If a file with the same name already exists, a numeric suffix is appended
/// (e.g. `file(1).txt`).
pub fn copy_to(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let file_name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let dest = unique_path(&dest_dir.join(file_name));

    if src.is_dir() {
        copy_dir_recursive(src, &dest)?;
    } else {
        fs::copy(src, &dest)?;
    }
    Ok(dest)
}

/// Move a single file or directory into `dest_dir`.
///
/// Tries `fs::rename` first (same-device fast path). Falls back to
/// copy-then-remove for cross-device moves.
pub fn move_to(src: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let file_name = src
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let dest = unique_path(&dest_dir.join(file_name));

    match fs::rename(src, &dest) {
        Ok(()) => Ok(dest),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            // Cross-device: copy then remove original.
            if src.is_dir() {
                copy_dir_recursive(src, &dest)?;
                fs::remove_dir_all(src)?;
            } else {
                fs::copy(src, &dest)?;
                fs::remove_file(src)?;
            }
            Ok(dest)
        }
        Err(e) => Err(e),
    }
}

/// Delete a file or directory (recursively).
pub fn delete(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Batch-copy a list of paths into `dest_dir`.
pub fn copy_batch(sources: &[PathBuf], dest_dir: &Path) -> FileOpResult {
    run_batch(sources, |src| {
        copy_to(src, dest_dir).map(|_| ())
    })
}

/// Batch-move a list of paths into `dest_dir`.
pub fn move_batch(sources: &[PathBuf], dest_dir: &Path) -> FileOpResult {
    run_batch(sources, |src| {
        move_to(src, dest_dir).map(|_| ())
    })
}

/// Process a `text/uri-list` drop payload: parse URIs and copy files.
pub fn handle_uri_drop(uri_list: &str, dest_dir: &Path) -> FileOpResult {
    let paths = parse_uri_list(uri_list);
    copy_batch(&paths, dest_dir)
}

// ── helpers ──

fn run_batch<F>(sources: &[PathBuf], mut op: F) -> FileOpResult
where
    F: FnMut(&Path) -> io::Result<()>,
{
    let mut done = 0usize;
    for src in sources {
        match op(src) {
            Ok(()) => done += 1,
            Err(e) => return FileOpResult::Partial(done, e),
        }
    }
    FileOpResult::Done(done)
}

/// Generate a unique path by appending `(N)` before the extension if needed.
fn unique_path(candidate: &Path) -> PathBuf {
    if !candidate.exists() {
        return candidate.to_path_buf();
    }
    let stem = candidate
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = candidate.extension().map(|e| e.to_string_lossy());
    let parent = candidate.parent().unwrap_or(Path::new("."));

    for n in 1u32.. {
        let new_name = match &ext {
            Some(e) => format!("{}({}).{}", stem, n, e),
            None => format!("{}({})", stem, n),
        };
        let p = parent.join(&new_name);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uri_list_basic() {
        let input = "file:///home/user/doc.txt\r\nfile:///tmp/image.png\r\n";
        let paths = parse_uri_list(input);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/doc.txt"));
        assert_eq!(paths[1], PathBuf::from("/tmp/image.png"));
    }

    #[test]
    fn parse_uri_list_with_comments_and_spaces() {
        let input = "# comment\nfile:///home/user/my%20file.txt\n";
        let paths = parse_uri_list(input);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/home/user/my file.txt"));
    }

    #[test]
    fn parse_uri_list_ignores_non_file() {
        let input = "http://example.com\nfile:///ok.txt\n";
        let paths = parse_uri_list(input);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/ok.txt"));
    }

    #[test]
    fn percent_decode_japanese() {
        // "テスト" in percent-encoded UTF-8
        let encoded = "/%E3%83%86%E3%82%B9%E3%83%88.txt";
        let decoded = percent_decode(encoded);
        assert_eq!(decoded, "/テスト.txt");
    }

    #[test]
    fn unique_path_no_conflict() {
        // Non-existent path returns as-is
        let p = PathBuf::from("/tmp/__nonexistent_test_file_12345__.txt");
        assert_eq!(unique_path(&p), p);
    }
}
