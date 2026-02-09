// 关闭控制台窗口 (仅 Release 模式且无调试输出时建议开启，此处暂保留以便调试)
// #![windows_subsystem = "windows"]

mod config;
mod ssh_core;
mod utils;

use clap::Parser;
use slint::{ModelRc, SharedString, VecModel, Weak};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use config::{AppConfig, ServerConfig};
use ssh_core::{FileTransfer, SshUploader};

slint::include_modules!();

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 要传输的文件路径 (可选，支持右键菜单传入)
    #[arg(value_name = "FILE")]
    file: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // 加载配置
    let config = AppConfig::load()?;
    let config = Arc::new(config);

    // 初始化 UI
    let ui = AppWindow::new()?;
    let ui_handle = ui.as_weak();

    // 设置初始状态
    let servers: Vec<SharedString> = config
        .servers
        .iter()
        .map(|s| SharedString::from(&s.name))
        .collect();
    ui.set_servers(ModelRc::new(VecModel::from(servers)));

    // 如果命令行有文件参数，设置 UI
    let initial_file = if let Some(path_str) = args.file {
        if let Ok(abs_path) = utils::normalize_path(&path_str) {
            ui.set_file_path(SharedString::from(abs_path.to_string_lossy().to_string()));
            Some(abs_path)
        } else {
            ui.set_file_path(SharedString::from(path_str));
            None
        }
    } else {
        None
    };

    // 如果有文件，准备好 context
    let selected_file = Arc::new(Mutex::new(initial_file));

    // 绑定开始上传事件
    let config_clone = config.clone();
    let _ = selected_file.clone(); // 保留引用虽然未直接使用，或者直接删除。这里虽然提示unused，但保留着也没坏处，或者直接删了。
                                   // User warning said: help: if this is intentional, prefix it with an underscore: `_selected_file_clone`
                                   // I will simply generally remove selected_file_clone as it is not used in the closure.
    let ui_handle_clone = ui_handle.clone();

    ui.on_start_upload(move |server_index| {
        let ui = ui_handle_clone.unwrap();

        // 获取当前文件路径 (以 UI 显示为准，如果支持拖拽的话)
        // 目前简单起见，使用命令行传入的或者默认的
        // 实际上应该允许 UI 选择文件，但 Slint 标准库目前没有文件选择对话框
        // 这里假设主要通过右键菜单使用

        let file_path_str = ui.get_file_path();
        if file_path_str == "未选择文件" {
            ui.set_status_log("请先选择文件 (目前仅支持通过命令行或右键菜单传入)".into());
            return;
        }

        let local_path = PathBuf::from(file_path_str.as_str());

        // 检查文件是否存在
        if let Err(e) = utils::ensure_file_exists(&local_path) {
            ui.set_status_log(format!("错误: {}", e).into());
            return;
        }

        // 获取服务器配置
        if server_index < 0 || server_index as usize >= config_clone.servers.len() {
            ui.set_status_log("无效的服务器选择".into());
            return;
        }
        let server_config = config_clone.servers[server_index as usize].clone();

        // 更新 UI 状态
        ui.set_is_uploading(true);
        ui.set_progress(0.0);
        ui.set_status_log(
            format!(
                "正在连接到 {} ({}:{})...",
                server_config.name, server_config.host, server_config.port
            )
            .into(),
        );

        let ui_handle_thread = ui_handle_clone.clone();

        // 启动后台线程执行上传
        thread::spawn(move || {
            let result = execute_upload(server_config, local_path, ui_handle_thread.clone());

            // 任务结束，更新 UI
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_thread.upgrade() {
                    ui.set_is_uploading(false);
                    match result {
                        Ok(_) => {
                            ui.set_status_log("上传成功! 🎉".into());
                            ui.set_progress(1.0);
                        }
                        Err(e) => {
                            ui.set_status_log(format!("上传失败: {}", e).into());
                            // 保持进度条，或者重置？保持以便查看
                        }
                    }
                }
            });
        });
    });

    ui.run()?;
    Ok(())
}

fn execute_upload(
    config: ServerConfig,
    local_path: PathBuf,
    ui_handle: Weak<AppWindow>,
) -> anyhow::Result<()> {
    // 1. 连接
    let mut uploader = SshUploader::connect(&config)?;

    // 2. 准备远程路径
    let file_name = local_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("无效的文件名"))?;
    let remote_path = Path::new(&config.default_target_dir).join(file_name);

    // 更新 UI: 开始上传
    let ui_handle_copy = ui_handle.clone();
    let remote_path_clone = remote_path.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_handle_copy.upgrade() {
            ui.set_status_log(format!("正在上传至 {:?}...", remote_path_clone).into());
        }
    })
    .unwrap();

    // 3. 上传
    uploader.upload(&local_path, &remote_path, |progress| {
        let ui_handle_copy = ui_handle.clone();
        // 注意：这里可能会频繁调用，生产环境可能需要节流 (throttle)
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle_copy.upgrade() {
                ui.set_progress(progress);
            }
        });
    })?;

    Ok(())
}
