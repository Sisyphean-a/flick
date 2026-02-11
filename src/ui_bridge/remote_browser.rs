use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::AppConfig;
use crate::remote_fs;
use crate::ssh_core::SshUploader;
use crate::AppWindow;
use crate::FileEntry;

use super::explorer::format_size;

/// 远程文件浏览器状态
pub(crate) struct RemoteState {
    pub current_path: String,
    pub uploader: Option<SshUploader>,
    pub selected_indices: HashSet<usize>,
    /// 缓存当前目录的条目(用于双击导航)
    pub cached_entries: Vec<remote_fs::RemoteEntry>,
    pub sort_field: String,
    pub sort_ascending: bool,
    pub filter_text: String,
    pub last_clicked_index: Option<usize>,
}

fn remote_entries_to_ui(
    entries: &[remote_fs::RemoteEntry],
    selected: &HashSet<usize>,
) -> Vec<FileEntry> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| FileEntry {
            name: SharedString::from(&e.name),
            is_dir: e.is_dir,
            size: SharedString::from(format_size(e.size, e.is_dir)),
            modified: SharedString::from(&e.modified),
            selected: selected.contains(&i),
        })
        .collect()
}

fn sort_remote_entries(entries: &mut Vec<remote_fs::RemoteEntry>, field: &str, ascending: bool) {
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

pub(crate) fn refresh_remote_dir(
    state: &Arc<Mutex<RemoteState>>,
    ui_handle: &slint::Weak<AppWindow>,
    path: &str,
) {
    let s = state.lock().unwrap();
    let uploader = match &s.uploader {
        Some(u) => u,
        None => return,
    };
    let selected = s.selected_indices.clone();
    let sort_field = s.sort_field.clone();
    let sort_asc = s.sort_ascending;
    let filter = s.filter_text.clone();

    let mut entries =
        remote_fs::list_dir_sftp(uploader, path).unwrap_or_default();
    sort_remote_entries(&mut entries, &sort_field, sort_asc);

    if !filter.is_empty() {
        let lower = filter.to_lowercase();
        entries.retain(|e| e.name.to_lowercase().contains(&lower));
    }

    let ui_entries = remote_entries_to_ui(&entries, &selected);
    let path_owned = path.to_string();
    drop(s);

    let mut s = state.lock().unwrap();
    s.current_path = path_owned.clone();
    s.cached_entries = entries;
    let file_count = s.cached_entries.len() as i32;
    let selected_count = s.selected_indices.len() as i32;
    drop(s);

    if let Some(ui) = ui_handle.upgrade() {
        ui.set_remote_path(SharedString::from(&path_owned));
        ui.set_remote_files(ModelRc::new(VecModel::from(ui_entries)));
        ui.set_remote_file_count(file_count);
        ui.set_remote_selected_count(selected_count);
    }
}

pub(crate) fn bind(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    remote_state: Arc<Mutex<RemoteState>>,
) {
    bind_remote_connect(ui, config, remote_state.clone());
    bind_remote_disconnect(ui, remote_state.clone());
    bind_remote_navigate(ui, remote_state.clone());
    bind_remote_go_up(ui, remote_state.clone());
    bind_remote_file_clicked(ui, remote_state.clone());
    bind_remote_double_click(ui, remote_state.clone());
    bind_remote_refresh(ui, remote_state.clone());
    bind_remote_select_all(ui, remote_state.clone());
    bind_remote_mkdir(ui, remote_state.clone());
    bind_remote_delete_selected(ui, remote_state.clone());
    bind_remote_rename(ui, remote_state.clone());
    bind_remote_sort_changed(ui, remote_state.clone());
    bind_remote_file_clicked_ex(ui, remote_state.clone());
    bind_remote_filter_changed(ui, remote_state);
}

fn bind_remote_connect(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_connect(move |server_index| {
        let config_guard = config.lock().unwrap();
        if server_index < 0
            || server_index as usize >= config_guard.servers.len()
        {
            return;
        }
        let server_config =
            config_guard.servers[server_index as usize].clone();
        drop(config_guard);

        if let Some(ui) = ui_handle.upgrade() {
            ui.set_remote_connecting(true);
            ui.set_remote_status("正在连接...".into());
        }

        let ui_h = ui_handle.clone();
        let st = state.clone();
        thread::spawn(move || {
            let (result, _logs) =
                SshUploader::connect_with_log(&server_config);

            let default_dir =
                server_config.default_target_dir.clone();

            match result {
                Ok(uploader) => {
                    let entries = remote_fs::list_dir_sftp(
                        &uploader, &default_dir,
                    )
                    .unwrap_or_default();

                    let ui_entries = remote_entries_to_ui(&entries, &HashSet::new());

                    let mut s = st.lock().unwrap();
                    s.current_path = default_dir.clone();
                    s.uploader = Some(uploader);
                    s.cached_entries = entries;
                    s.selected_indices.clear();
                    drop(s);

                    let _ = slint::invoke_from_event_loop(
                        move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_remote_connecting(false);
                                ui.set_remote_connected(true);
                                ui.set_remote_path(
                                    SharedString::from(&default_dir),
                                );
                                ui.set_remote_files(ModelRc::new(
                                    VecModel::from(ui_entries),
                                ));
                                ui.set_remote_status("".into());
                            }
                        },
                    );
                }
                Err(e) => {
                    let msg = format!("连接失败: {}", e);
                    let _ = slint::invoke_from_event_loop(
                        move || {
                            if let Some(ui) = ui_h.upgrade() {
                                ui.set_remote_connecting(false);
                                ui.set_remote_connected(false);
                                ui.set_remote_status(
                                    SharedString::from(&msg),
                                );
                            }
                        },
                    );
                }
            }
        });
    });
}

fn bind_remote_delete_selected(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_delete_selected(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let s = state.lock().unwrap();
            let count = s.selected_indices.len();
            drop(s);
            if count == 0 {
                return;
            }
            ui.set_confirm_title(SharedString::from("确认删除"));
            ui.set_confirm_message(SharedString::from(
                format!("确定要删除远程的 {} 个项目吗？此操作不可撤销。", count),
            ));
            ui.set_confirm_action(SharedString::from("remote-delete"));
            ui.set_show_confirm(true);
        }
    });
}
fn bind_remote_disconnect(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_disconnect(move || {
        let mut s = state.lock().unwrap();
        s.uploader = None;
        s.cached_entries.clear();
        s.selected_indices.clear();
        s.current_path = "/".to_string();
        drop(s);

        if let Some(ui) = ui_handle.upgrade() {
            ui.set_remote_connected(false);
            ui.set_remote_path("/".into());
            ui.set_remote_files(ModelRc::new(VecModel::from(
                Vec::<FileEntry>::new(),
            )));
            ui.set_remote_status("".into());
        }
    });
}

fn bind_remote_navigate(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_navigate(move |path_str| {
        let mut s = state.lock().unwrap();
        s.selected_indices.clear();
        drop(s);
        refresh_remote_dir(
            &state,
            &ui_handle,
            path_str.as_str(),
        );
    });
}

fn bind_remote_go_up(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_go_up(move || {
        let current = {
            let s = state.lock().unwrap();
            s.current_path.clone()
        };
        let parent = if current == "/" {
            "/".to_string()
        } else {
            let p = std::path::Path::new(&current);
            p.parent()
                .map(|pp| pp.to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string())
        };
        let parent = if parent.is_empty() {
            "/".to_string()
        } else {
            parent
        };
        let mut s = state.lock().unwrap();
        s.selected_indices.clear();
        drop(s);
        refresh_remote_dir(&state, &ui_handle, &parent);
    });
}

fn bind_remote_file_clicked(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_file_clicked(move |index| {
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
                ui.get_remote_files().set_row_data(idx, file_entry);
                ui.set_remote_selected_count(sel_count);
            }
        }
    });
}

fn bind_remote_double_click(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_file_double_clicked(move |index| {
        let (is_dir, name, current) = {
            let s = state.lock().unwrap();
            match s.cached_entries.get(index as usize) {
                Some(e) => {
                    (e.is_dir, e.name.clone(), s.current_path.clone())
                }
                None => return,
            }
        };
        if is_dir {
            let new_path = if current.ends_with('/') {
                format!("{}{}", current, name)
            } else {
                format!("{}/{}", current, name)
            };
            let mut s = state.lock().unwrap();
            s.selected_indices.clear();
            drop(s);
            refresh_remote_dir(&state, &ui_handle, &new_path);
        }
    });
}

fn bind_remote_refresh(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_refresh(move || {
        let current = {
            let s = state.lock().unwrap();
            s.current_path.clone()
        };
        refresh_remote_dir(&state, &ui_handle, &current);
    });
}

fn bind_remote_select_all(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_select_all(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let total = s.cached_entries.len();
            if s.selected_indices.len() == total {
                s.selected_indices.clear();
            } else {
                s.selected_indices = (0..total).collect();
            }
            let selected = s.selected_indices.clone();
            let entries = s.cached_entries.clone();
            drop(s);

            let ui_entries = remote_entries_to_ui(&entries, &selected);
            ui.set_remote_files(ModelRc::new(VecModel::from(ui_entries)));
        }
    });
}

fn bind_remote_mkdir(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_mkdir(move |dir_name| {
        let s = state.lock().unwrap();
        let uploader = match &s.uploader {
            Some(u) => u,
            None => return,
        };
        let current = s.current_path.clone();
        let new_dir = if current.ends_with('/') {
            format!("{}{}", current, dir_name)
        } else {
            format!("{}/{}", current, dir_name)
        };
        if let Err(e) = remote_fs::remote_mkdir(uploader, &new_dir) {
            eprintln!("创建目录失败: {}", e);
            return;
        }
        drop(s);
        refresh_remote_dir(&state, &ui_handle, &current);
        let _ = state.lock().map(|mut s| {
            s.selected_indices.clear();
        });
    });
}

fn bind_remote_rename(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_rename(move |index, new_name| {
        let s = state.lock().unwrap();
        let uploader = match &s.uploader {
            Some(u) => u,
            None => return,
        };
        let current = s.current_path.clone();
        let entry = match s.cached_entries.get(index as usize) {
            Some(e) => e,
            None => return,
        };
        let old_path = if current.ends_with('/') {
            format!("{}{}", current, entry.name)
        } else {
            format!("{}/{}", current, entry.name)
        };
        let new_path = if current.ends_with('/') {
            format!("{}{}", current, new_name)
        } else {
            format!("{}/{}", current, new_name)
        };
        if let Err(e) = remote_fs::remote_rename(uploader, &old_path, &new_path) {
            eprintln!("重命名失败: {}", e);
            return;
        }
        drop(s);
        refresh_remote_dir(&state, &ui_handle, &current);
    });
}

fn bind_remote_sort_changed(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_sort_changed(move |field| {
        let current = {
            let mut s = state.lock().unwrap();
            if s.sort_field == field.as_str() {
                s.sort_ascending = !s.sort_ascending;
            } else {
                s.sort_field = field.to_string();
                s.sort_ascending = true;
            }
            s.current_path.clone()
        };
        refresh_remote_dir(&state, &ui_handle, &current);
    });
}

fn bind_remote_file_clicked_ex(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_file_clicked_ex(move |index, ctrl, shift| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let idx = index as usize;
            let total = s.cached_entries.len();
            if idx >= total {
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

            let ui_entries = remote_entries_to_ui(&entries, &selected);
            ui.set_remote_files(ModelRc::new(VecModel::from(ui_entries)));
            ui.set_remote_selected_count(sel_count);
        }
    });
}

fn bind_remote_filter_changed(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_filter_changed(move |text| {
        let current = {
            let mut s = state.lock().unwrap();
            s.filter_text = text.to_string();
            s.selected_indices.clear();
            s.current_path.clone()
        };
        refresh_remote_dir(&state, &ui_handle, &current);
    });
}