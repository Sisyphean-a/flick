use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::app::services::explorer_service;
use crate::domain::config::{AppConfig, Bookmark};
use crate::domain::ports::ConfigRepository;
use crate::domain::transfer::TransferQueue;
use crate::AppWindow;
use crate::BookmarkEntry;

pub mod bookmarks_bindings;
pub mod local_bindings;
pub mod remote_bindings;
pub mod transfer_bindings;

use self::local_bindings::LocalState;
use self::remote_bindings::RemoteState;

pub(crate) fn format_size(bytes: u64, is_dir: bool) -> String {
    if is_dir {
        return "-".to_string();
    }
    if bytes == 0 {
        return "0 B".to_string();
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

pub fn bind(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    repo: Arc<dyn ConfigRepository + Send + Sync>,
) {
    let local_state = Arc::new(Mutex::new(LocalState {
        current_path: local_bindings::default_start_dir(),
        selected_indices: HashSet::new(),
        cached_entries: Vec::new(),
        sort_field: "name".to_string(),
        sort_ascending: true,
        filter_text: String::new(),
        last_clicked_index: None,
    }));

    let remote_state = Arc::new(Mutex::new(RemoteState {
        current_path: "/".to_string(),
        uploader: None,
        selected_indices: HashSet::new(),
        cached_entries: Vec::new(),
        sort_field: "name".to_string(),
        sort_ascending: true,
        filter_text: String::new(),
        last_clicked_index: None,
    }));

    let transfer_queue = Arc::new(Mutex::new(TransferQueue::new()));

    // 本地回调
    local_bindings::bind(ui, local_state.clone());

    // 远程回调
    remote_bindings::bind(ui, config.clone(), remote_state.clone());

    // 传输队列回调
    transfer_bindings::bind(
        ui,
        local_state.clone(),
        remote_state.clone(),
        transfer_queue,
    );

    // 确认对话框回调
    bind_confirm_accepted(ui, local_state.clone(), remote_state.clone());

    // 书签回调
    bind_bookmarks(
        ui,
        config.clone(),
        repo,
        local_state,
        remote_state,
    );
    refresh_bookmarks(ui, &config);
}

fn bind_confirm_accepted(
    ui: &AppWindow,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_confirm_accepted(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let action = ui.get_confirm_action().to_string();
            match action.as_str() {
                "local-delete" => {
                    do_local_delete(&ui, &local_state);
                }
                "remote-delete" => {
                    do_remote_delete(&ui, &remote_state);
                }
                _ => {}
            }
        }
    });
}

fn do_local_delete(ui: &AppWindow, state: &Arc<Mutex<LocalState>>) {
    let s = state.lock().unwrap();
    let to_delete: Vec<_> = s
        .selected_indices
        .iter()
        .filter_map(|&i| s.cached_entries.get(i))
        .map(|e| (e.path.clone(), e.is_dir))
        .collect();
    drop(s);
    for (path, is_dir) in &to_delete {
        let result = if *is_dir {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        if let Err(e) = result {
            eprintln!("删除失败 {:?}: {}", path, e);
        }
    }
    let mut s = state.lock().unwrap();
    s.selected_indices.clear();
    drop(s);
    local_bindings::refresh_local(ui, state);
}

fn do_remote_delete(ui: &AppWindow, state: &Arc<Mutex<RemoteState>>) {
    let s = state.lock().unwrap();
    let uploader = match &s.uploader {
        Some(u) => u,
        None => return,
    };
    let current = s.current_path.clone();
    let to_delete: Vec<_> = s
        .selected_indices
        .iter()
        .filter_map(|&i| s.cached_entries.get(i))
        .map(|e| {
            let full = if current.ends_with('/') {
                format!("{}{}", current, e.name)
            } else {
                format!("{}/{}", current, e.name)
            };
            (full, e.is_dir)
        })
        .collect();
    for (path, is_dir) in &to_delete {
        if let Err(e) = crate::infra::remote_fs::remote_remove(uploader, path, *is_dir) {
            eprintln!("删除失败 {}: {}", path, e);
        }
    }
    drop(s);
    let mut s = state.lock().unwrap();
    s.selected_indices.clear();
    drop(s);
    let ui_weak = ui.as_weak();
    remote_bindings::refresh_remote_dir(state, &ui_weak, &current);
}

fn refresh_bookmarks(ui: &AppWindow, config: &Arc<Mutex<AppConfig>>) {
    let cfg = config.lock().unwrap();
    let entries: Vec<BookmarkEntry> = cfg
        .bookmarks
        .iter()
        .map(|b| BookmarkEntry {
            name: SharedString::from(&b.name),
            path: SharedString::from(&b.path),
            side: SharedString::from(&b.side),
        })
        .collect();
    drop(cfg);
    ui.set_bookmarks(ModelRc::new(VecModel::from(entries)));
}

fn bind_bookmarks(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    repo: Arc<dyn ConfigRepository + Send + Sync>,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
) {
    // 添加书签
    let cfg_add = config.clone();
    let repo_add = repo.clone();
    let ui_add = ui.as_weak();
    ui.on_add_bookmark(move |name, path, side| {
        let mut cfg = cfg_add.lock().unwrap();
        // 避免重复
        if explorer_service::dedup_bookmark(
            &cfg.bookmarks,
            path.as_str(),
            side.as_str(),
        ) {
            return;
        }
        cfg.bookmarks.push(Bookmark {
            name: name.to_string(),
            path: path.to_string(),
            side: side.to_string(),
        });
        let _ = repo_add.save(&cfg);
        let entries: Vec<BookmarkEntry> = cfg.bookmarks.iter().map(|b| BookmarkEntry {
            name: SharedString::from(&b.name),
            path: SharedString::from(&b.path),
            side: SharedString::from(&b.side),
        }).collect();
        drop(cfg);
        if let Some(ui) = ui_add.upgrade() {
            ui.set_bookmarks(ModelRc::new(VecModel::from(entries)));
        }
    });

    // 删除书签
    let cfg_rm = config.clone();
    let repo_rm = repo;
    let ui_rm = ui.as_weak();
    ui.on_remove_bookmark(move |index| {
        let mut cfg = cfg_rm.lock().unwrap();
        let idx = index as usize;
        if idx < cfg.bookmarks.len() {
            cfg.bookmarks.remove(idx);
            let _ = repo_rm.save(&cfg);
        }
        let entries: Vec<BookmarkEntry> = cfg.bookmarks.iter().map(|b| BookmarkEntry {
            name: SharedString::from(&b.name),
            path: SharedString::from(&b.path),
            side: SharedString::from(&b.side),
        }).collect();
        drop(cfg);
        if let Some(ui) = ui_rm.upgrade() {
            ui.set_bookmarks(ModelRc::new(VecModel::from(entries)));
        }
    });

    // 跳转书签
    let cfg_goto = config;
    let ui_goto = ui.as_weak();
    ui.on_goto_bookmark(move |index| {
        let cfg = cfg_goto.lock().unwrap();
        let idx = index as usize;
        let bm = match cfg.bookmarks.get(idx) {
            Some(b) => b.clone(),
            None => return,
        };
        drop(cfg);

        match bm.side.as_str() {
            "local" => {
                let mut s = local_state.lock().unwrap();
                s.current_path = std::path::PathBuf::from(&bm.path);
                s.selected_indices.clear();
                drop(s);
                if let Some(ui) = ui_goto.upgrade() {
                    local_bindings::refresh_local(&ui, &local_state);
                }
            }
            "remote" => {
                let mut s = remote_state.lock().unwrap();
                s.selected_indices.clear();
                drop(s);
                if let Some(ui) = ui_goto.upgrade() {
                    let weak = ui.as_weak();
                    remote_bindings::refresh_remote_dir(
                        &remote_state,
                        &weak,
                        &bm.path,
                    );
                }
            }
            _ => {}
        }
    });
}

