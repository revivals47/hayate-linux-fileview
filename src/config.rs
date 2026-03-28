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
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
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
        let content = format!(
            "show_hidden = {}\nsort_column = \"{}\"\nsort_order = \"{}\"\nview_mode = \"{}\"\nwindow_width = {}\nwindow_height = {}\n",
            self.show_hidden, self.sort_column, self.sort_order, self.view_mode,
            self.window_width, self.window_height,
        );
        let _ = std::fs::write(&path, content);
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
        }
    }
}
