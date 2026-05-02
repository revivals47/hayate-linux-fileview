//! Application state for the file viewer.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::TextEngine;

use crate::entry::{DirEntry, SortColumn, SortOrder, read_dir_sorted};

/// Maximum navigation history entries (back/forward stacks).
const MAX_NAV_HISTORY: usize = 50;

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
    pub(crate) back_stack: Vec<PathBuf>,    // capped at MAX_NAV_HISTORY
    pub(crate) forward_stack: Vec<PathBuf>, // capped at MAX_NAV_HISTORY
    pub(crate) quit_flag: Option<Rc<Cell<bool>>>,
    pub(crate) title_buffer: Option<Rc<RefCell<Option<String>>>>,
    pub(crate) system_clipboard: Option<Rc<RefCell<Option<String>>>>,
    pub(crate) paste_request: Option<Rc<Cell<bool>>>,
    pub(crate) sidebar_ratio: Rc<Cell<Option<f32>>>,
    pub(crate) preview_ratio: Rc<Cell<Option<f32>>>,
    pub(crate) fs_watcher: Option<crate::watcher::FsWatcher>,
}

impl FileViewState {
    pub(crate) fn new_with_config(
        path: PathBuf, engine: Rc<RefCell<TextEngine>>,
        sort_column: SortColumn, sort_order: SortOrder,
        show_hidden: bool, view_mode: ViewMode,
    ) -> Self {
        let (entries, last_error) = match read_dir_sorted(&path, show_hidden, sort_column, sort_order) {
            Ok(e) => (e, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let fs_watcher = Some(crate::watcher::FsWatcher::new(&path));
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
            view_mode,
            search_query: None,
            filtered_indices: None,
            last_error,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            quit_flag: None,
            title_buffer: None,
            system_clipboard: None,
            paste_request: None,
            sidebar_ratio: Rc::new(Cell::new(None)),
            preview_ratio: Rc::new(Cell::new(None)),
            fs_watcher,
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

    pub(crate) fn update_title(&self) {
        if let Some(buf) = &self.title_buffer {
            buf.borrow_mut()
                .replace(format!("Hayate — {}", self.current_path.display()));
        }
    }

    pub(crate) fn navigate(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.last_error = Some(format!("Cannot open: {}", path.display()));
            return;
        }
        self.back_stack.push(self.current_path.clone());
        // Cap navigation history to prevent unbounded memory growth
        if self.back_stack.len() > MAX_NAV_HISTORY {
            self.back_stack.drain(..self.back_stack.len() - MAX_NAV_HISTORY);
        }
        self.forward_stack.clear();
        self.current_path = path;
        self.refresh();
        self.update_title();
        if let Some(ref mut w) = self.fs_watcher { w.watch(&self.current_path); }
    }

    pub(crate) fn go_back(&mut self) {
        if let Some(prev) = self.back_stack.pop() {
            self.forward_stack.push(self.current_path.clone());
            self.current_path = prev;
            self.refresh();
            self.update_title();
            if let Some(ref mut w) = self.fs_watcher { w.watch(&self.current_path); }
        }
    }

    pub(crate) fn go_forward(&mut self) {
        if let Some(next) = self.forward_stack.pop() {
            self.back_stack.push(self.current_path.clone());
            self.current_path = next;
            self.refresh();
            self.update_title();
            if let Some(ref mut w) = self.fs_watcher { w.watch(&self.current_path); }
        }
    }

    pub(crate) fn can_go_back(&self) -> bool { !self.back_stack.is_empty() }
    pub(crate) fn can_go_forward(&self) -> bool { !self.forward_stack.is_empty() }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_state(path: &std::path::Path) -> FileViewState {
        let engine = Rc::new(RefCell::new(TextEngine::new()));
        FileViewState::new_with_config(
            path.to_path_buf(), engine,
            SortColumn::Name, SortOrder::Asc, false, ViewMode::Detail,
        )
    }

    #[test]
    fn navigate_to_valid_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("a.txt"), "").unwrap();

        let mut s = make_state(dir.path());
        s.navigate(sub.clone());
        assert_eq!(s.current_path, sub);
        assert!(s.entries.iter().any(|e| e.name == "a.txt"));
        assert!(s.last_error.is_none());
    }

    #[test]
    fn navigate_to_nonexistent_sets_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = make_state(dir.path());
        s.navigate(dir.path().join("no_such_dir"));
        assert!(s.last_error.is_some());
        assert_eq!(s.current_path, dir.path());
    }

    #[test]
    fn go_back_and_forward() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("child");
        fs::create_dir(&sub).unwrap();

        let mut s = make_state(dir.path());
        let original = s.current_path.clone();
        s.navigate(sub.clone());
        assert_eq!(s.current_path, sub);

        s.go_back();
        assert_eq!(s.current_path, original);
        assert!(s.can_go_forward());

        s.go_forward();
        assert_eq!(s.current_path, sub);
        assert!(!s.can_go_forward());
    }

    #[test]
    fn refresh_picks_up_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = make_state(dir.path());
        let before = s.entries.len();

        fs::write(dir.path().join("new_file.txt"), "hello").unwrap();
        s.refresh();
        assert!(s.entries.len() > before);
        assert!(s.entries.iter().any(|e| e.name == "new_file.txt"));
    }

    #[test]
    fn select_single_and_toggle() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "").unwrap();
        fs::write(dir.path().join("b"), "").unwrap();

        let mut s = make_state(dir.path());
        s.select_single(0);
        assert!(s.is_selected(0));
        assert!(!s.is_selected(1));
        assert_eq!(s.cursor, Some(0));

        s.toggle_select(1);
        assert!(s.is_selected(1));
        s.toggle_select(1);
        assert!(!s.is_selected(1));
    }

    #[test]
    fn select_range() {
        let dir = tempfile::tempdir().unwrap();
        for name in &["a", "b", "c", "d"] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        let mut s = make_state(dir.path());
        s.select_range(1, 3);
        assert!(!s.is_selected(0));
        assert!(s.is_selected(1));
        assert!(s.is_selected(2));
        assert!(s.is_selected(3));
    }

    #[test]
    fn set_sort_toggles_order() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("zzz"), "").unwrap();
        fs::write(dir.path().join("aaa"), "").unwrap();

        let mut s = make_state(dir.path());
        // Initial: Name Asc. Same column → toggle to Desc.
        s.set_sort(SortColumn::Name);
        assert_eq!(s.sort_order, SortOrder::Desc);
        let first_desc = s.entries[0].name.clone();

        s.set_sort(SortColumn::Name);
        assert_eq!(s.sort_order, SortOrder::Asc);
        let first_asc = s.entries[0].name.clone();
        assert_ne!(first_desc, first_asc);
    }

    #[test]
    fn select_all_and_indices() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x"), "").unwrap();
        fs::write(dir.path().join("y"), "").unwrap();

        let mut s = make_state(dir.path());
        s.select_all();
        assert_eq!(s.selected_indices().len(), s.entries.len());
    }
}
