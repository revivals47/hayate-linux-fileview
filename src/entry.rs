//! Directory entry types, sorting, and filesystem reading.

use std::path::Path;
use unicode_width::UnicodeWidthChar;

// ── Sort types ──

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SortColumn { Name, Size, Modified }

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SortOrder { Asc, Desc }

impl SortOrder {
    pub(crate) fn toggle(self) -> Self { match self { Self::Asc => Self::Desc, Self::Desc => Self::Asc } }
    pub(crate) fn indicator(self) -> &'static str { match self { Self::Asc => "▲", Self::Desc => "▼" } }
}

// ── Directory entry ──

pub(crate) struct DirEntry {
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    pub(crate) is_symlink: bool,
    pub(crate) size: u64,
    pub(crate) modified: Option<std::time::SystemTime>,
}

/// Format a byte count as a human-readable string (B / KB / MB / GB / TB).
pub(crate) fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes < 1024u64 * 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.1} TB", bytes as f64 / (1024.0 * 1024.0 * 1024.0 * 1024.0))
    }
}

/// Truncate a string to fit within `max_width` display columns (CJK-aware).
pub(crate) fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > max_width { break; }
        width += cw;
        result.push(ch);
    }
    result
}

/// Pad a string with spaces to reach `target` display columns (CJK-aware).
fn pad_to_width(s: &str, target: usize) -> String {
    let w: usize = s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    if w >= target { s.to_string() } else { format!("{}{}", s, " ".repeat(target - w)) }
}

impl DirEntry {
    pub(crate) fn format_size(&self) -> String {
        format_size(self.size)
    }

    pub(crate) fn format_modified(&self) -> String {
        match &self.modified {
            Some(t) => {
                let secs = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let time_t = secs as libc::time_t;
                let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
                unsafe { libc::localtime_r(&time_t, &mut tm) };
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday,
                    tm.tm_hour,
                    tm.tm_min,
                )
            }
            None => "           —".to_string(),
        }
    }

    pub(crate) fn display_line(&self) -> String {
        let icon = if self.is_symlink { "🔗  " } else if self.is_dir { "📁  " } else { "    " };
        let name = if self.is_dir { format!("{}/", self.name) } else { self.name.clone() };
        let truncated = truncate_to_width(&name, 20);
        let padded = pad_to_width(&truncated, 20);
        let size_str = if self.is_dir { String::new() } else { self.format_size() };
        format!("{}{} {:>8} {}", icon, padded, size_str, self.format_modified())
    }
}

/// Read directory entries, sorted: directories first, then by column/order.
pub(crate) fn read_dir_sorted(path: &Path, show_hidden: bool, col: SortColumn, ord: SortOrder) -> Result<Vec<DirEntry>, String> {
    let rd = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') { continue; }
        let sym_meta = entry.path().symlink_metadata().ok();
        let is_symlink = sym_meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
        // Follow symlinks for actual metadata (size, is_dir, etc.)
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        entries.push(DirEntry { name, is_dir, is_symlink, size, modified });
    }
    entries.sort_by(|a, b| {
        let dir_cmp = b.is_dir.cmp(&a.is_dir);
        let field_cmp = match col {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::Modified => a.modified.cmp(&b.modified),
        };
        let ordered = if ord == SortOrder::Desc { field_cmp.reverse() } else { field_cmp };
        dir_cmp.then(ordered)
    });
    Ok(entries)
}
