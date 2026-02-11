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
    pub sort_field: String,
    pub sort_ascending: bool,
    pub filter_text: String,
    pub last_clicked_index: Option<usize>,
}

pub(crate) fn default_start_dir() -> PathBuf {
    PathBuf::from("")
}

fn sort_local_entries(entries: &mut Vec<local_fs::LocalEntry>, field: &str, ascending: bool) {
    entries.sort_by(|a, b| {
        let dir_ord = b.is_dir.cmp(&a.is_dir);
        if dir_ord != std::cmp::Ordering::Equal {
            return dir_ord;
        }
        let ord = match field {
            "size" => a.size.cmp(&b.size),
            "modified" => a.modified.cmp(&b.modified),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        };
        if ascending { ord } else { ord.reverse() }
    });
}

pub(crate) fn refresh_local(ui: &AppWindow, state: &Arc<Mutex<LocalState>>) {
    let s = state.lock().unwrap();
    let path = s.current_path.clone();
    let selected = s.selected_indices.clone();
    let sort_field = s.sort_field.clone();
    let sort_asc = s.sort_ascending;
    let filter = s.filter_text.clone();
    drop(s);

    let mut entries = match local_fs::list_dir(&path) {
        Ok(e) => e,
        Err(_) => return,
    };

    sort_local_entries(&mut entries, &sort_field, sort_asc);

    if !filter.is_empty() {
        let lower = filter.to_lowercase();
        entries.retain(|e| e.name.to_lowercase().contains(&lower));
    }

    let file_entries: Vec<FileEntry> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| FileEntry {
            name: SharedString::from(&e.name),
            is_dir: e.is_dir,
            size: SharedString::from(format_size(e.size, e.is_dir)),
            modified: SharedString::from(&e.modified),
            selected: selected.contains(&i),
        })
        .collect();

    let mut s = state.lock().unwrap();
    s.cached_entries = entries;
    let file_count = s.cached_entries.len() as i32;
    let selected_count = s.selected_indices.len() as i32;
    drop(s);

    let display_path = if path.as_os_str().is_empty() {
        "我的电脑".to_string()
    } else {
        path.to_string_lossy().to_string()
    };

    ui.set_local_path(SharedString::from(display_path));
    ui.set_local_files(ModelRc::new(VecModel::from(file_entries)));
    ui.set_local_file_count(file_count);
    ui.set_local_selected_count(selected_count);
}

pub(crate) fn bind(ui: &AppWindow, local_state: Arc<Mutex<LocalState>>) {
    refresh_local(ui, &local_state);

    bind_local_navigate(ui, local_state.clone());
    bind_local_go_up(ui, local_state.clone());
    bind_local_file_clicked(ui, local_state.clone());
    bind_local_double_click(ui, local_state.clone());
    bind_local_refresh(ui, local_state.clone());
    bind_local_select_all(ui, local_state.clone());
    bind_local_mkdir(ui, local_state.clone());
    bind_local_delete_selected(ui, local_state.clone());
    bind_local_rename(ui, local_state.clone());
    bind_local_sort_changed(ui, local_state.clone());
    bind_local_file_clicked_ex(ui, local_state.clone());
    bind_local_filter_changed(ui, local_state);
}

fn bind_local_navigate(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_navigate(move |path_str| {
        let path = if path_str == "我的电脑" {
            PathBuf::from("")
        } else {
            PathBuf::from(path_str.as_str())
        };
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
            } else if !s.current_path.as_os_str().is_empty() {
                // If no parent and not at root (My Computer), go to My Computer
                s.current_path = PathBuf::from("");
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
                let sel_count = s.selected_indices.len() as i32;
                drop(s);
                let file_entry = FileEntry {
                    name: SharedString::from(&entry.name),
                    is_dir: entry.is_dir,
                    size: SharedString::from(format_size(entry.size, entry.is_dir)),
                    modified: SharedString::from(&entry.modified),
                    selected: is_selected,
                };
                ui.get_local_files().set_row_data(idx, file_entry);
                ui.set_local_selected_count(sel_count);
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

fn bind_local_mkdir(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_mkdir(move |dir_name| {
        if let Some(ui) = ui_handle.upgrade() {
            let s = state.lock().unwrap();
            if s.current_path.as_os_str().is_empty() {
                return;
            }
            let new_dir = s.current_path.join(dir_name.as_str());
            drop(s);
            if let Err(e) = std::fs::create_dir(&new_dir) {
                eprintln!("创建目录失败: {}", e);
                return;
            }
            let mut s = state.lock().unwrap();
            s.selected_indices.clear();
            drop(s);
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_delete_selected(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_delete_selected(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let s = state.lock().unwrap();
            let count = s.selected_indices.len();
            drop(s);
            if count == 0 {
                return;
            }
            ui.set_confirm_title(SharedString::from("确认删除"));
            ui.set_confirm_message(SharedString::from(
                format!("确定要删除选中的 {} 个项目吗？此操作不可撤销。", count),
            ));
            ui.set_confirm_action(SharedString::from("local-delete"));
            ui.set_show_confirm(true);
        }
    });
}

fn bind_local_rename(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_rename(move |index, new_name| {
        if let Some(ui) = ui_handle.upgrade() {
            let s = state.lock().unwrap();
            let entry = match s.cached_entries.get(index as usize) {
                Some(e) => e,
                None => return,
            };
            let old_path = entry.path.clone();
            let new_path = old_path.parent().map(|p| p.join(new_name.as_str()));
            drop(s);
            if let Some(new_path) = new_path {
                if let Err(e) = std::fs::rename(&old_path, &new_path) {
                    eprintln!("重命名失败: {}", e);
                    return;
                }
            }
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_sort_changed(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_sort_changed(move |field| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            if s.sort_field == field.as_str() {
                s.sort_ascending = !s.sort_ascending;
            } else {
                s.sort_field = field.to_string();
                s.sort_ascending = true;
            }
            drop(s);
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_file_clicked_ex(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_file_clicked_ex(move |index, ctrl, shift| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let idx = index as usize;
            let total = s.cached_entries.len();
            if idx >= total {
                return;
            }

            // 如果没有按 Ctrl 或 Shift，这里直接忽略
            // 因为普通点击已经由 bind_local_file_clicked 处理
            if !ctrl && !shift {
                return;
            }

            // 如果没有按 Ctrl 或 Shift，这里直接忽略
            // 因为普通点击已经由 bind_local_file_clicked 处理
            if !ctrl && !shift {
                return;
            }

            if shift {
                let anchor = s.last_clicked_index.unwrap_or(0);
                let (start, end) = if anchor <= idx {
                    (anchor, idx)
                } else {
                    (idx, anchor)
                };
                s.selected_indices.clear();
                for i in start..=end {
                    s.selected_indices.insert(i);
                }
            } else if ctrl {
                if s.selected_indices.contains(&idx) {
                    s.selected_indices.remove(&idx);
                } else {
                    s.selected_indices.insert(idx);
                }
                s.last_clicked_index = Some(idx);
            } else {
                s.selected_indices.clear();
                s.selected_indices.insert(idx);
                s.last_clicked_index = Some(idx);
            }

            let selected = s.selected_indices.clone();
            let entries = s.cached_entries.clone();
            let sel_count = selected.len() as i32;
            drop(s);

            let ui_entries: Vec<FileEntry> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| FileEntry {
                    name: SharedString::from(&e.name),
                    is_dir: e.is_dir,
                    size: SharedString::from(format_size(e.size, e.is_dir)),
                    modified: SharedString::from(&e.modified),
                    selected: selected.contains(&i),
                })
                .collect();
            ui.set_local_files(ModelRc::new(VecModel::from(ui_entries)));
            ui.set_local_selected_count(sel_count);
        }
    });
}

fn bind_local_filter_changed(ui: &AppWindow, state: Arc<Mutex<LocalState>>) {
    let ui_handle = ui.as_weak();
    ui.on_local_filter_changed(move |text| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            s.filter_text = text.to_string();
            s.selected_indices.clear();
            drop(s);
            refresh_local(&ui, &state);
        }
    });
}
