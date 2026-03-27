//! Application state for the file viewer.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use hayate_ui::render::TextEngine;

use crate::entry::{DirEntry, SortColumn, SortOrder, read_dir_sorted};

pub(crate) struct FileViewState {
    pub(crate) current_path: PathBuf,
    pub(crate) show_hidden: bool,
    pub(crate) entries: Vec<DirEntry>,
    pub(crate) selected_index: Option<usize>,
    pub(crate) sort_column: SortColumn,
    pub(crate) sort_order: SortOrder,
    pub(crate) engine: Rc<RefCell<TextEngine>>,
}

impl FileViewState {
    pub(crate) fn new(path: PathBuf, engine: Rc<RefCell<TextEngine>>) -> Self {
        let show_hidden = false;
        let sort_column = SortColumn::Name;
        let sort_order = SortOrder::Asc;
        let entries = read_dir_sorted(&path, show_hidden, sort_column, sort_order);
        Self {
            current_path: path,
            show_hidden,
            entries,
            selected_index: None,
            sort_column,
            sort_order,
            engine,
        }
    }

    pub(crate) fn refresh(&mut self) {
        self.entries = read_dir_sorted(&self.current_path, self.show_hidden, self.sort_column, self.sort_order);
        self.selected_index = None;
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
        self.current_path = path;
        self.refresh();
    }

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
}
