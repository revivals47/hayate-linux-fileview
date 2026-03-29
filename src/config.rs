//! Persistent configuration: load/save a simple TOML-like key=value file.

use std::path::PathBuf;

use crate::entry::{SortColumn, SortOrder};
use crate::state::ViewMode;

pub(crate) struct Config {
    pub(crate) show_hidden: bool,
    pub(crate) sort_column: String,
    pub(crate) sort_order: String,
    pub(crate) view_mode: String,
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
    pub(crate) sidebar_ratio: Option<f32>,
    pub(crate) preview_ratio: Option<f32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            show_hidden: false,
            sort_column: "name".into(),
            sort_order: "asc".into(),
            view_mode: "detail".into(),
            window_width: 750,
            window_height: 450,
            sidebar_ratio: None,
            preview_ratio: None,
        }
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/hayate-fileview/config.toml")
}

impl Config {
    pub(crate) fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => Self::parse(&s),
            Err(_) => Self::default(),
        }
    }

    pub(crate) fn parse(content: &str) -> Self {
        let mut cfg = Self::default();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim().trim_matches('"');
                match key {
                    "show_hidden" => cfg.show_hidden = val == "true",
                    "sort_column" => cfg.sort_column = val.to_string(),
                    "sort_order" => cfg.sort_order = val.to_string(),
                    "view_mode" => cfg.view_mode = val.to_string(),
                    "window_width" => cfg.window_width = val.parse().unwrap_or(750),
                    "window_height" => cfg.window_height = val.parse().unwrap_or(450),
                    "sidebar_ratio" => cfg.sidebar_ratio = val.parse().ok().filter(|&v: &f32| (0.0..=1.0).contains(&v)),
                    "preview_ratio" => cfg.preview_ratio = val.parse().ok().filter(|&v: &f32| (0.0..=1.0).contains(&v)),
                    _ => {}
                }
            }
        }
        cfg
    }

    pub(crate) fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, self.format());
    }

    pub(crate) fn format(&self) -> String {
        let mut s = format!(
            "show_hidden = {}\nsort_column = \"{}\"\nsort_order = \"{}\"\nview_mode = \"{}\"\nwindow_width = {}\nwindow_height = {}\n",
            self.show_hidden, self.sort_column, self.sort_order, self.view_mode,
            self.window_width, self.window_height,
        );
        if let Some(r) = self.sidebar_ratio { s.push_str(&format!("sidebar_ratio = {:.4}\n", r)); }
        if let Some(r) = self.preview_ratio { s.push_str(&format!("preview_ratio = {:.4}\n", r)); }
        s
    }

    pub(crate) fn to_sort_column(&self) -> SortColumn {
        match self.sort_column.as_str() {
            "size" => SortColumn::Size,
            "modified" => SortColumn::Modified,
            _ => SortColumn::Name,
        }
    }

    pub(crate) fn to_sort_order(&self) -> SortOrder {
        match self.sort_order.as_str() {
            "desc" => SortOrder::Desc,
            _ => SortOrder::Asc,
        }
    }

    pub(crate) fn to_view_mode(&self) -> ViewMode {
        match self.view_mode.as_str() {
            "list" => ViewMode::List,
            "compact" => ViewMode::Compact,
            _ => ViewMode::Detail,
        }
    }

    pub(crate) fn from_state(state: &crate::state::FileViewState) -> Self {
        Self {
            show_hidden: state.show_hidden,
            sort_column: match state.sort_column {
                SortColumn::Name => "name",
                SortColumn::Size => "size",
                SortColumn::Modified => "modified",
            }.into(),
            sort_order: match state.sort_order {
                SortOrder::Asc => "asc",
                SortOrder::Desc => "desc",
            }.into(),
            view_mode: match state.view_mode {
                ViewMode::Detail => "detail",
                ViewMode::List => "list",
                ViewMode::Compact => "compact",
            }.into(),
            window_width: 750, // TODO: track actual window size
            window_height: 450,
            sidebar_ratio: state.sidebar_ratio.get(),
            preview_ratio: state.preview_ratio.get(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = Config::default();
        assert!(!cfg.show_hidden);
        assert_eq!(cfg.sort_column, "name");
        assert_eq!(cfg.sort_order, "asc");
        assert_eq!(cfg.view_mode, "detail");
        assert_eq!(cfg.window_width, 750);
        assert_eq!(cfg.window_height, 450);
        assert_eq!(cfg.sidebar_ratio, None);
        assert_eq!(cfg.preview_ratio, None);
    }

    #[test]
    fn roundtrip_format_parse() {
        let cfg = Config {
            show_hidden: true,
            sort_column: "size".into(),
            sort_order: "desc".into(),
            view_mode: "compact".into(),
            window_width: 1024,
            window_height: 768,
            sidebar_ratio: Some(0.2),
            preview_ratio: Some(0.35),
        };
        let text = cfg.format();
        let loaded = Config::parse(&text);
        assert!(loaded.show_hidden);
        assert_eq!(loaded.sort_column, "size");
        assert_eq!(loaded.sort_order, "desc");
        assert_eq!(loaded.view_mode, "compact");
        assert_eq!(loaded.window_width, 1024);
        assert_eq!(loaded.window_height, 768);
        assert!((loaded.sidebar_ratio.unwrap() - 0.2).abs() < 0.001);
        assert!((loaded.preview_ratio.unwrap() - 0.35).abs() < 0.001);
    }

    #[test]
    fn ratio_none_roundtrip() {
        let cfg = Config { sidebar_ratio: None, preview_ratio: None, ..Config::default() };
        let loaded = Config::parse(&cfg.format());
        assert_eq!(loaded.sidebar_ratio, None);
        assert_eq!(loaded.preview_ratio, None);
    }

    #[test]
    fn ratio_out_of_range_ignored() {
        let text = "sidebar_ratio = 1.5\npreview_ratio = -0.1\n";
        let cfg = Config::parse(text);
        assert_eq!(cfg.sidebar_ratio, None);
        assert_eq!(cfg.preview_ratio, None);
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let text = "# comment\n\nshow_hidden = true\n";
        let cfg = Config::parse(text);
        assert!(cfg.show_hidden);
        assert_eq!(cfg.sort_column, "name"); // default preserved
    }

    #[test]
    fn unknown_keys_ignored() {
        let text = "unknown_key = foo\nshow_hidden = true\n";
        let cfg = Config::parse(text);
        assert!(cfg.show_hidden);
    }
}
