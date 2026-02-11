use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::config::AppConfig;
use crate::transfer::TransferQueue;
use crate::AppWindow;

use super::local_browser::{self, LocalState};
use super::remote_browser::{self, RemoteState};
use super::transfer_manager;

pub(crate) fn format_size(bytes: u64) -> String {
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

pub fn bind(ui: &AppWindow, config: Arc<Mutex<AppConfig>>) {
    let local_state = Arc::new(Mutex::new(LocalState {
        current_path: local_browser::default_start_dir(),
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

    // 本地回调
    local_browser::bind(ui, local_state.clone());

    // 远程回调
    remote_browser::bind(ui, config, remote_state.clone());

    // 传输队列回调
    transfer_manager::bind(ui, local_state, remote_state, transfer_queue);
}
