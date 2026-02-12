use slint::{ComponentHandle, SharedString, Weak};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::app::services::quick_upload_service;
use crate::domain::config::{AppConfig, ServerConfig};
use crate::AppWindow;

pub fn bind(ui: &AppWindow, config: Arc<Mutex<AppConfig>>) {
    bind_pick_file(ui);
    bind_server_selected(ui, config.clone());
    bind_start_upload(ui, config);
}

fn bind_pick_file(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_pick_file(move || {
        if let Some(ui) = ui_handle.upgrade() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                ui.set_file_path(SharedString::from(
                    path.to_string_lossy().as_ref(),
                ));
            }
        }
    });
}

fn bind_server_selected(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
) {
    let ui_handle = ui.as_weak();
    ui.on_server_selected(move |index| {
        let guard = match config.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if index >= 0 && (index as usize) < guard.servers.len() {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_target_dir(SharedString::from(
                    &guard.servers[index as usize].default_target_dir,
                ));
            }
        }
    });
}

fn bind_start_upload(ui: &AppWindow, config: Arc<Mutex<AppConfig>>) {
    let ui_handle = ui.as_weak();
    ui.on_start_upload(move |server_index| {
        let ui = match ui_handle.upgrade() {
            Some(ui) => ui,
            None => return,
        };

        let file_path_str = ui.get_file_path();
        if file_path_str == "未选择文件" {
            ui.set_status_log("请先选择文件".into());
            return;
        }

        let local_path = PathBuf::from(file_path_str.as_str());
        if let Err(e) =
            quick_upload_service::validate_upload_path(&local_path)
        {
            ui.set_status_log(format!("错误: {}", e).into());
            return;
        }

        let config_guard = match config.lock() {
            Ok(g) => g,
            Err(_) => {
                ui.set_status_log("内部错误: 配置锁定失败".into());
                return;
            }
        };
        if server_index < 0
            || server_index as usize >= config_guard.servers.len()
        {
            ui.set_status_log("无效的服务器选择".into());
            return;
        }
        let mut server_config =
            config_guard.servers[server_index as usize].clone();
        drop(config_guard);

        server_config.default_target_dir =
            ui.get_target_dir().to_string();

        ui.set_is_uploading(true);
        ui.set_progress(0.0);
        ui.set_status_log(
            format!(
                "正在连接到 {} ({}:{})...",
                server_config.name,
                server_config.host,
                server_config.port
            )
            .into(),
        );

        let ui_handle_thread = ui_handle.clone();
        thread::spawn(move || {
            let result = execute_upload(
                server_config,
                local_path,
                ui_handle_thread.clone(),
            );
            finish_upload(ui_handle_thread, result);
        });
    });
}

fn execute_upload(
    config: ServerConfig,
    local_path: PathBuf,
    ui_handle: Weak<AppWindow>,
) -> anyhow::Result<()> {
    let file_name = local_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("无效的文件名"))?;
    let remote_path =
        Path::new(&config.default_target_dir).join(file_name);

    let ui_copy = ui_handle.clone();
    let rp = remote_path.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_copy.upgrade() {
            ui.set_status_log(
                format!("正在上传至 {:?}...", rp).into(),
            );
        }
    })
    .ok();

    quick_upload_service::execute_upload(config, local_path, |progress| {
        let ui_copy = ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_copy.upgrade() {
                ui.set_progress(progress);
            }
        });
    })?;

    Ok(())
}

fn finish_upload(
    ui_handle: Weak<AppWindow>,
    result: anyhow::Result<()>,
) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_is_uploading(false);
            match result {
                Ok(_) => {
                    ui.set_status_log("上传成功! 🎉".into());
                    ui.set_progress(1.0);
                }
                Err(e) => {
                    ui.set_status_log(
                        format!("上传失败: {}", e).into(),
                    );
                }
            }
        }
    });
}

