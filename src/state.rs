//! Application state for the file viewer.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::TextEngine;

use crate::entry::{DirEntry, SortColumn, SortOrder, read_dir_sorted};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ViewMode { Detail, List, Compact }

pub(crate) struct FileViewState {
    pub(crate) current_path: PathBuf,
    pub(crate) show_hidden: bool,
    pub(crate) entries: Vec<DirEntry>,
    pub(crate) selected: HashSet<usize>,
    pub(crate) anchor: Option<usize>,
    pub(crate) cursor: Option<usize>,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_order: SortOrder,
    pub(crate) engine: Rc<RefCell<TextEngine>>,
    pub(crate) view_mode: ViewMode,
    pub(crate) search_query: Option<String>,
    pub(crate) filtered_indices: Option<Vec<usize>>,
    pub(crate) last_error: Option<String>,
}

impl FileViewState {
    pub(crate) fn new(path: PathBuf, engine: Rc<RefCell<TextEngine>>) -> Self {
        let show_hidden = false;
        let sort_column = SortColumn::Name;
        let sort_order = SortOrder::Asc;
        let (entries, last_error) = match read_dir_sorted(&path, show_hidden, sort_column, sort_order) {
            Ok(e) => (e, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Self {
            current_path: path,
            show_hidden,
            entries,
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            sort_column,
            sort_order,
            engine,
            view_mode: ViewMode::Detail,
            search_query: None,
            filtered_indices: None,
            last_error,
        }
    }

    pub(crate) fn refresh(&mut self) {
        match read_dir_sorted(&self.current_path, self.show_hidden, self.sort_column, self.sort_order) {
            Ok(entries) => {
                self.entries = entries;
                self.last_error = None;
            }
            Err(e) => {
                self.entries = Vec::new();
                self.last_error = Some(e);
            }
        }
        self.clear_selection();
        self.search_query = None;
        self.filtered_indices = None;
    }

    pub(crate) fn is_selected(&self, idx: usize) -> bool {
        self.selected.contains(&idx)
    }

    pub(crate) fn select_single(&mut self, idx: usize) {
        self.selected.clear();
        self.selected.insert(idx);
        self.anchor = Some(idx);
        self.cursor = Some(idx);
    }

    pub(crate) fn toggle_select(&mut self, idx: usize) {
        if self.selected.contains(&idx) {
            self.selected.remove(&idx);
        } else {
            self.selected.insert(idx);
        }
        self.anchor = Some(idx);
        self.cursor = Some(idx);
    }

    pub(crate) fn select_range(&mut self, from: usize, to: usize) {
        self.selected.clear();
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        for i in lo..=hi {
            self.selected.insert(i);
        }
        self.cursor = Some(to);
    }

    pub(crate) fn select_all(&mut self) {
        self.selected = (0..self.entries.len()).collect();
        self.cursor = if self.entries.is_empty() { None } else { Some(0) };
    }

    pub(crate) fn selected_indices(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.selected.iter().copied().collect();
        v.sort_unstable();
        v
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.cursor = None;
    }

    pub(crate) fn set_sort(&mut self, col: SortColumn) {
        if self.sort_column == col {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_column = col;
            self.sort_order = SortOrder::Asc;
        }
        self.refresh();
    }

    pub(crate) fn navigate(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.last_error = Some(format!("Cannot open: {}", path.display()));
            return;
        }
        self.current_path = path;
        self.refresh();
    }

    pub(crate) fn clear_error(&mut self) { self.last_error = None; }

    pub(crate) fn go_parent(&mut self) {
        if let Some(parent) = self.current_path.parent() {
            let parent = parent.to_path_buf();
            self.navigate(parent);
        }
    }

    pub(crate) fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }

    pub(crate) fn cycle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Detail => ViewMode::List,
            ViewMode::List => ViewMode::Compact,
            ViewMode::Compact => ViewMode::Detail,
        };
    }

    pub(crate) fn set_search(&mut self, query: Option<String>) {
        self.search_query = query;
        self.update_filter();
    }

    pub(crate) fn update_filter(&mut self) {
        match &self.search_query {
            None | Some(_) if self.search_query.as_deref() == Some("") => {
                self.filtered_indices = None;
            }
            Some(q) => {
                let lower_q = q.to_lowercase();
                self.filtered_indices = Some(
                    self.entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.name.to_lowercase().contains(&lower_q))
                        .map(|(i, _)| i)
                        .collect(),
                );
            }
            _ => {}
        }
    }

    pub(crate) fn visible_entries(&self) -> Vec<(usize, &DirEntry)> {
        match &self.filtered_indices {
            None => self.entries.iter().enumerate().collect(),
            Some(indices) => indices
                .iter()
                .filter_map(|&i| self.entries.get(i).map(|e| (i, e)))
                .collect(),
        }
    }
}
