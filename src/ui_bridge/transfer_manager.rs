use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::ssh_core::{FileTransfer, SshUploader};
use crate::transfer::{Direction, TransferQueue, TransferStatus};
use crate::AppWindow;
use crate::TransferEntry;

use super::local_browser::{self, LocalState};
use super::remote_browser::{self, RemoteState};

pub(crate) fn bind(
    ui: &AppWindow,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
    transfer_queue: Arc<Mutex<TransferQueue>>,
) {
    bind_upload_selected(
        ui,
        local_state.clone(),
        remote_state.clone(),
        transfer_queue.clone(),
    );
    bind_download_selected(
        ui,
        local_state.clone(),
        remote_state.clone(),
        transfer_queue.clone(),
    );
    bind_clear_completed_transfers(ui, transfer_queue.clone());
    bind_retry_transfer(ui, local_state, remote_state, transfer_queue.clone());
    start_transfer_queue_sync(ui, transfer_queue);
}

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

            let queue_clone = queue.clone();
            let cfg = uploader_config.clone();
            let rs_clone = remote_state.clone();
            let ui_h = ui_handle.clone();
            let rp = remote_path.clone();
            thread::spawn(move || {
                let mut uploader = match SshUploader::connect(&cfg) {
                    Ok(u) => u,
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("连接失败: {}", e));
                        return;
                    }
                };

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
                        let rs = rs_clone.clone();
                        let uh = ui_h.clone();
                        let path = rp.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            remote_browser::refresh_remote_dir(&rs, &uh, &path);
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

            let queue_clone = queue.clone();
            let cfg = uploader_config.clone();
            let ls_clone = local_state.clone();
            let ui_h = ui_handle.clone();
            thread::spawn(move || {
                let mut uploader = match SshUploader::connect(&cfg) {
                    Ok(u) => u,
                    Err(e) => {
                        let mut q = queue_clone.lock().unwrap();
                        q.mark_failed(task_id, format!("连接失败: {}", e));
                        return;
                    }
                };

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
                        let ls = ls_clone.clone();
                        let uh = ui_h.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = uh.upgrade() {
                                local_browser::refresh_local(&ui, &ls);
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

fn bind_retry_transfer(
    ui: &AppWindow,
    local_state: Arc<Mutex<LocalState>>,
    remote_state: Arc<Mutex<RemoteState>>,
    queue: Arc<Mutex<TransferQueue>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_retry_transfer(move |task_id| {
        let task_id = task_id as usize;
        let mut q = queue.lock().unwrap();
        if !q.retry(task_id) {
            return;
        }
        let task = match q.get_task(task_id) {
            Some(t) => t,
            None => return,
        };
        drop(q);

        let uploader_config = {
            let rs = remote_state.lock().unwrap();
            match &rs.uploader {
                Some(u) => u.config().clone(),
                None => return,
            }
        };

        let queue_clone = queue.clone();
        let rs_clone = remote_state.clone();
        let ls_clone = local_state.clone();
        let ui_h = ui_handle.clone();
        let local_path = task.local_path.clone();
        let remote_path = task.remote_path.clone();
        let is_dir = local_path.is_dir();
        let direction = task.direction.clone();

        thread::spawn(move || {
            let mut uploader = match SshUploader::connect(&uploader_config) {
                Ok(u) => u,
                Err(e) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.mark_failed(task_id, format!("连接失败: {}", e));
                    return;
                }
            };

            let progress_cb = |progress: f32| {
                let q_clone = queue_clone.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let mut q = q_clone.lock().unwrap();
                    q.update_progress(task_id, progress);
                });
            };

            let result = match direction {
                Direction::Upload => {
                    if is_dir {
                        uploader.upload_dir(&local_path, Path::new(&remote_path), progress_cb)
                    } else {
                        uploader.upload(&local_path, Path::new(&remote_path), progress_cb)
                    }
                }
                Direction::Download => {
                    if is_dir {
                        uploader.download_dir(Path::new(&remote_path), &local_path, progress_cb)
                    } else {
                        uploader.download(Path::new(&remote_path), &local_path, progress_cb)
                    }
                }
            };

            match result {
                Ok(_) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.mark_completed(task_id);
                    match direction {
                        Direction::Upload => {
                            let rs = rs_clone.clone();
                            let uh = ui_h.clone();
                            let rp = rs.lock().unwrap().current_path.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                remote_browser::refresh_remote_dir(&rs, &uh, &rp);
                            });
                        }
                        Direction::Download => {
                            let ls = ls_clone.clone();
                            let uh = ui_h.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = uh.upgrade() {
                                    local_browser::refresh_local(&ui, &ls);
                                }
                            });
                        }
                    }
                }
                Err(e) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.mark_failed(task_id, format!("{}", e));
                }
            }
        });
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
                        TransferStatus::Pending => ("pending".to_string(), String::new()),
                        TransferStatus::InProgress => ("progress".to_string(), String::new()),
                        TransferStatus::Completed => ("done".to_string(), String::new()),
                        TransferStatus::Failed(e) => ("failed".to_string(), e.clone()),
                    };

                    let direction = match t.direction {
                        Direction::Upload => "上传",
                        Direction::Download => "下载",
                    };

                    let (speed, eta) = if t.status == TransferStatus::InProgress {
                        if let Some(started) = t.started_at {
                            let elapsed = started.elapsed().as_secs_f64();
                            if elapsed > 0.5 && t.progress > 0.0 {
                                let bytes_done = (t.size as f64) * (t.progress as f64);
                                let bps = bytes_done / elapsed;
                                let speed_str = format_speed(bps);
                                let remaining = if t.progress < 1.0 {
                                    let remaining_bytes = (t.size as f64) * (1.0 - t.progress as f64);
                                    let secs = (remaining_bytes / bps) as u64;
                                    format_eta(secs)
                                } else {
                                    String::new()
                                };
                                (speed_str, remaining)
                            } else {
                                (String::new(), String::new())
                            }
                        } else {
                            (String::new(), String::new())
                        }
                    } else {
                        (String::new(), String::new())
                    };

                    TransferEntry {
                        task_id: t.id as i32,
                        file_name: SharedString::from(&t.file_name),
                        direction: SharedString::from(direction),
                        progress: t.progress,
                        status: SharedString::from(&status_text),
                        error_msg: SharedString::from(&error_msg),
                        speed: SharedString::from(&speed),
                        eta: SharedString::from(&eta),
                    }
                })
                .collect();

            ui.set_transfer_tasks(ModelRc::new(VecModel::from(transfer_entries)));
            ui.set_has_transfer_tasks(!tasks.is_empty());
        }
    });
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{:.0} B/s", bytes_per_sec)
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    }
}

fn format_eta(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}
