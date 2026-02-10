use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::AppConfig;
use crate::local_fs;
use crate::remote_fs;
use crate::ssh_core::{FileTransfer, SshUploader};
use crate::transfer::{Direction, TransferQueue, TransferStatus};
use crate::AppWindow;
use crate::{FileEntry, TransferEntry};

/// 本地文件浏览器状态
struct LocalState {
    current_path: PathBuf,
    selected_indices: HashSet<usize>,
    cached_entries: Vec<local_fs::LocalEntry>,
}

/// 远程文件浏览器状态
struct RemoteState {
    current_path: String,
    uploader: Option<SshUploader>,
    selected_indices: HashSet<usize>,
    /// 缓存当前目录的条目(用于双击导航)
    cached_entries: Vec<remote_fs::RemoteEntry>,
}

pub fn bind(ui: &AppWindow, config: Arc<Mutex<AppConfig>>) {
    let local_state = Arc::new(Mutex::new(LocalState {
        current_path: default_start_dir(),
        selected_indices: HashSet::new(),
        cached_entries: Vec::new(),
    }));

    let remote_state = Arc::new(Mutex::new(RemoteState {
        current_path: "/".to_string(),
        uploader: None,
        selected_indices: HashSet::new(),
        cached_entries: Vec::new(),
    }));

    let transfer_queue = Arc::new(Mutex::new(TransferQueue::new()));

    // 初始加载本地
    refresh_local(ui, &local_state);

    // 本地回调
    bind_local_navigate(ui, local_state.clone());
    bind_local_go_up(ui, local_state.clone());
    bind_local_file_clicked(ui, local_state.clone());
    bind_local_double_click(ui, local_state.clone());
    bind_local_refresh(ui, local_state.clone());
    bind_local_select_all(ui, local_state.clone());

    // 远程回调
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

    // 传输队列回调
    bind_upload_selected(ui, local_state.clone(), remote_state.clone(), transfer_queue.clone());
    bind_download_selected(ui, local_state, remote_state, transfer_queue.clone());
    bind_clear_completed_transfers(ui, transfer_queue.clone());

    // 定时同步传输队列状态到 UI
    start_transfer_queue_sync(ui, transfer_queue);
}

fn default_start_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    for unit in UNITS {
        if size < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} TB", size)
}

fn refresh_local(ui: &AppWindow, state: &Arc<Mutex<LocalState>>) {
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

    // 更新缓存
    let mut s = state.lock().unwrap();
    s.cached_entries = entries;
    drop(s);

    ui.set_local_path(SharedString::from(
        path.to_string_lossy().as_ref(),
    ));
    ui.set_local_files(ModelRc::new(VecModel::from(file_entries)));
}

fn bind_local_navigate(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
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

fn bind_local_go_up(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
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

fn bind_local_file_clicked(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_local_file_clicked(move |index| {
        if let Some(ui) = ui_handle.upgrade() {
            let mut s = state.lock().unwrap();
            let idx = index as usize;
            
            // 切换选中状态
            let is_selected = if s.selected_indices.contains(&idx) {
                s.selected_indices.remove(&idx);
                false
            } else {
                s.selected_indices.insert(idx);
                true
            };
            
            // 获取缓存数据并更新单行 Model，避免重建组件导致丢失双击事件
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

fn bind_local_double_click(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
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

fn bind_local_refresh(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_local_refresh(move || {
        if let Some(ui) = ui_handle.upgrade() {
            refresh_local(&ui, &state);
        }
    });
}

fn bind_local_select_all(
    ui: &AppWindow,
    state: Arc<Mutex<LocalState>>,
) {
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

// ========== 远程文件浏览器 ==========

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
            size: SharedString::from(format_size(e.size)),
            modified: SharedString::from(&e.modified),
            selected: selected.contains(&i),
        })
        .collect()
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

        // 标记连接中
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
                    // 列出默认目录
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

fn refresh_remote_dir(
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

    let entries =
        remote_fs::list_dir_sftp(uploader, path).unwrap_or_default();
    let ui_entries = remote_entries_to_ui(&entries, &selected);
    let path_owned = path.to_string();
    drop(s);

    let mut s = state.lock().unwrap();
    s.current_path = path_owned.clone();
    s.cached_entries = entries;
    drop(s);

    if let Some(ui) = ui_handle.upgrade() {
        ui.set_remote_path(SharedString::from(&path_owned));
        ui.set_remote_files(ModelRc::new(VecModel::from(ui_entries)));
    }
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
        // 确保 Unix 路径不为空
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
                drop(s);
                let file_entry = FileEntry {
                    name: SharedString::from(&entry.name),
                    is_dir: entry.is_dir,
                    size: SharedString::from(format_size(entry.size)),
                    modified: SharedString::from(&entry.modified),
                    selected: is_selected,
                };
                ui.get_remote_files().set_row_data(idx, file_entry);
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
        // 清除选择状态（refresh 后 selected 可能不对应）
        let _ = state.lock().map(|mut s| {
            s.selected_indices.clear();
        });
    });
}

fn bind_remote_delete_selected(
    ui: &AppWindow,
    state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_remote_delete_selected(move || {
        let s = state.lock().unwrap();
        let uploader = match &s.uploader {
            Some(u) => u,
            None => return,
        };
        let current = s.current_path.clone();
        let to_delete: Vec<_> = s.selected_indices
            .iter()
            .filter_map(|&i| s.cached_entries.get(i))
            .map(|e| {
                let full_path = if current.ends_with('/') {
                    format!("{}{}", current, e.name)
                } else {
                    format!("{}/{}", current, e.name)
                };
                (full_path, e.is_dir)
            })
            .collect();
        for (path, is_dir) in &to_delete {
            if let Err(e) = remote_fs::remote_remove(uploader, path, *is_dir) {
                eprintln!("删除失败 {}: {}", path, e);
            }
        }
        drop(s);
        let mut s = state.lock().unwrap();
        s.selected_indices.clear();
        drop(s);
        refresh_remote_dir(&state, &ui_handle, &current);
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

// ========== 传输队列 ==========

fn bind_upload_selected(
    ui: &AppWindow,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
    queue: Arc<Mutex<TransferQueue>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_upload_selected(move || {
        let (local_files, remote_path, uploader_opt) = {
            let ls = local_state.lock().unwrap();
            let rs = remote_state.lock().unwrap();
            
            let files: Vec<_> = ls.selected_indices
                .iter()
                .filter_map(|&i| ls.cached_entries.get(i))
                .map(|e| (e.path.clone(), e.name.clone(), e.size, e.is_dir))
                .collect();
            
            (files, rs.current_path.clone(), rs.uploader.as_ref().map(|u| u.config().clone()))
        };

        if local_files.is_empty() {
            return;
        }

        let uploader_config = match uploader_opt {
            Some(cfg) => cfg,
            None => return,
        };

        // 加入队列
        for (local_path, file_name, size, is_dir) in local_files {
            let remote_file_path = if remote_path.ends_with('/') {
                format!("{}{}", remote_path, file_name)
            } else {
                format!("{}/{}", remote_path, file_name)
            };

            let task_id = {
                let mut q = queue.lock().unwrap();
                q.enqueue(
                    Direction::Upload,
                    local_path.clone(),
                    remote_file_path.clone(),
                    file_name.clone(),
                    size,
                )
            };

            // 启动后台上传线程
            let queue_clone = queue.clone();
            let cfg = uploader_config.clone();
            let rs_clone = remote_state.clone();
            let ui_h = ui_handle.clone();
            let rp = remote_path.clone();
            thread::spawn(move || {
                // 连接
                let mut uploader = match SshUploader::connect(&cfg) {
                    Ok(u) => u,
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("连接失败: {}", e));
                        return;
                    }
                };

                // 上传（文件或目录）
                let progress_cb = |progress: f32| {
                    let q_clone = queue_clone.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let mut q = q_clone.lock().unwrap();
                        q.update_progress(task_id, progress);
                    });
                };
                let result = if is_dir {
                    uploader.upload_dir(
                        &local_path,
                        Path::new(&remote_file_path),
                        progress_cb,
                    )
                } else {
                    uploader.upload(
                        &local_path,
                        Path::new(&remote_file_path),
                        progress_cb,
                    )
                };

                match result {
                    Ok(_) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_completed(task_id);
                        // 上传完成后刷新远程目录
                        let rs = rs_clone.clone();
                        let uh = ui_h.clone();
                        let path = rp.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            refresh_remote_dir(&rs, &uh, &path);
                        });
                    }
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("{}", e));
                    }
                }
            });
        }
    });
}

fn bind_download_selected(
    ui: &AppWindow,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
    queue: Arc<Mutex<TransferQueue>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_download_selected(move || {
        let (remote_files, local_path, uploader_opt) = {
            let rs = remote_state.lock().unwrap();
            let ls = local_state.lock().unwrap();

            let files: Vec<_> = rs.selected_indices
                .iter()
                .filter_map(|&i| rs.cached_entries.get(i))
                .map(|e| {
                    let remote_full_path = if rs.current_path.ends_with('/') {
                        format!("{}{}", rs.current_path, e.name)
                    } else {
                        format!("{}/{}", rs.current_path, e.name)
                    };
                    (remote_full_path, e.name.clone(), e.size, e.is_dir)
                })
                .collect();

            let local_dir = ls.current_path.clone();

            (files, local_dir, rs.uploader.as_ref().map(|u| u.config().clone()))
        };

        if remote_files.is_empty() {
            return;
        }

        let uploader_config = match uploader_opt {
            Some(cfg) => cfg,
            None => return,
        };

        // 加入队列
        for (remote_file_path, file_name, size, is_dir) in remote_files {
            let local_file_path = local_path.join(&file_name);

            let task_id = {
                let mut q = queue.lock().unwrap();
                q.enqueue(
                    Direction::Download,
                    local_file_path.clone(),
                    remote_file_path.clone(),
                    file_name.clone(),
                    size,
                )
            };

            // 启动后台下载线程
            let queue_clone = queue.clone();
            let cfg = uploader_config.clone();
            let ls_clone = local_state.clone();
            let ui_h = ui_handle.clone();
            thread::spawn(move || {
                // 连接
                let mut uploader = match SshUploader::connect(&cfg) {
                    Ok(u) => u,
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("连接失败: {}", e));
                        return;
                    }
                };

                // 下载（文件或目录）
                let progress_cb = |progress: f32| {
                    let q_clone = queue_clone.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let mut q = q_clone.lock().unwrap();
                        q.update_progress(task_id, progress);
                    });
                };
                let result = if is_dir {
                    uploader.download_dir(
                        Path::new(&remote_file_path),
                        &local_file_path,
                        progress_cb,
                    )
                } else {
                    uploader.download(
                        Path::new(&remote_file_path),
                        &local_file_path,
                        progress_cb,
                    )
                };

                match result {
                    Ok(_) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_completed(task_id);
                        // 下载完成后刷新本地目录
                        let ls = ls_clone.clone();
                        let uh = ui_h.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = uh.upgrade() {
                                refresh_local(&ui, &ls);
                            }
                        });
                    }
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("{}", e));
                    }
                }
            });
        }
    });
}

fn bind_clear_completed_transfers(
    ui: &AppWindow,
    queue: Arc<Mutex<TransferQueue>>,
) {
    ui.on_clear_completed_transfers(move || {
        let mut q = queue.lock().unwrap();
        q.clear_completed();
    });
}

fn start_transfer_queue_sync(
    ui: &AppWindow,
    queue: Arc<Mutex<TransferQueue>>,
) {
    let ui_handle = ui.as_weak();
    let timer = Timer::default();
    
    timer.start(TimerMode::Repeated, std::time::Duration::from_millis(200), move || {
        if let Some(ui) = ui_handle.upgrade() {
            let q = queue.lock().unwrap();
            let tasks = q.snapshot();
            drop(q);

            let transfer_entries: Vec<TransferEntry> = tasks
                .iter()
                .map(|t| {
                    let (status_text, error_msg) = match &t.status {
                        TransferStatus::Pending => ("等待中".to_string(), String::new()),
                        TransferStatus::InProgress => ("传输中".to_string(), String::new()),
                        TransferStatus::Completed => ("已完成".to_string(), String::new()),
                        TransferStatus::Failed(e) => ("失败".to_string(), e.clone()),
                    };

                    let direction = match t.direction {
                        Direction::Upload => "上传",
                        Direction::Download => "下载",
                    };

                    TransferEntry {
                        file_name: SharedString::from(&t.file_name),
                        direction: SharedString::from(direction),
                        progress: t.progress,
                        status: SharedString::from(&status_text),
                        error_msg: SharedString::from(&error_msg),
                    }
                })
                .collect();

            ui.set_transfer_tasks(ModelRc::new(VecModel::from(transfer_entries)));
            ui.set_has_transfer_tasks(!tasks.is_empty());
        }
    });
}