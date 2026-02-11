use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::local_fs;
use crate::AppWindow;
use crate::FileEntry;

use super::explorer::format_size;

/// 本地文件浏览器状态
pub(crate) struct LocalState {
    pub current_path: PathBuf,
    pub selected_indices: HashSet<usize>,
    pub cached_entries: Vec<local_fs::LocalEntry>,
}

pub(crate) fn default_start_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub(crate) fn refresh_local(ui: &AppWindow, state: &Arc<Mutex<LocalState>>) {
    let s = state.lock().unwrap();
    let path = s.current_path.clone();
    let selected = s.selected_indices.clone();
    drop(s);

    let entries = match local_fs::list_dir(&path) {
        Ok(e) => e,
        Err(_) => return,
    };

    let file_entries: Vec<FileEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| FileEntry {
            name: SharedString::from(&e.name),
            is_dir: e.is_dir,
            size: SharedString::from(format_size(e.size)),
            modified: SharedString::from(&e.modified),
            selected: selected.contains(&i),
        })
        .collect();

    let mut s = state.lock().unwrap();
    s.cached_entries = entries;
    drop(s);

    ui.set_local_path(SharedString::from(
        path.to_string_lossy().as_ref(),
    ));
    ui.set_local_files(ModelRc::new(VecModel::from(file_entries)));
}

pub(crate) fn bind(ui: &AppWindow, local_state: Arc<Mutex<LocalState>>) {
    refresh_local(ui, &local_state);

    bind_local_navigate(ui, local_state.clone());
    bind_local_go_up(ui, local_state.clone());
    bind_local_file_clicked(ui, local_state.clone());
    bind_local_double_click(ui, local_state.clone());
    bind_local_refresh(ui, local_state.clone());
    bind_local_select_all(ui, local_state);
}

fn bind_local_navigate(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_navigate(move |path_str| {
        let path = PathBuf::from(path_str.as_str());
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            s.current_path = path.clone();
            s.selected_indices.clear();
            drop(s);
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_go_up(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_go_up(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            if let Some(parent) = s.current_path.parent() {
                let parent = parent.to_path_buf();
                s.current_path = parent.clone();
                s.selected_indices.clear();
                drop(s);
                refresh_local(&ui, &state);
            }
        }
    });
}

fn bind_local_file_clicked(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_file_clicked(move |index| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let idx = index as usize;

            let is_selected = if s.selected_indices.contains(&idx) {
                s.selected_indices.remove(&idx);
                false
            } else {
                s.selected_indices.insert(idx);
                true
            };

            if let Some(entry) = s.cached_entries.get(idx).cloned() {
                drop(s);
                let file_entry = FileEntry {
                    name: SharedString::from(&entry.name),
                    is_dir: entry.is_dir,
                    size: SharedString::from(format_size(entry.size)),
                    modified: SharedString::from(&entry.modified),
                    selected: is_selected,
                };
                ui.get_local_files().set_row_data(idx, file_entry);
            }
        }
    });
}

fn bind_local_double_click(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_file_double_clicked(move |index| {
        if let Some(ui) = ui_handle.upgrade() {
            let s = state.lock().unwrap();
            if let Some(entry) = s.cached_entries.get(index as usize) {
                if entry.is_dir {
                    let new_path = entry.path.clone();
                    drop(s);
                    let mut s = state.lock().unwrap();
                    s.current_path = new_path;
                    s.selected_indices.clear();
                    drop(s);
                    refresh_local(&ui, &state);
                }
            }
        }
    });
}

fn bind_local_refresh(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_refresh(move || {
        if let Some(ui) = ui_handle.upgrade() {
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_select_all(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_select_all(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let total = s.cached_entries.len();
            if s.selected_indices.len() == total {
                s.selected_indices.clear();
            } else {
                s.selected_indices = (0..total).collect();
            }
            drop(s);
            refresh_local(&ui, &state);
        }
    });
}
