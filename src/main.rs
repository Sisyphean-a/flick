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
    let config = Arc::new(Mutex::new(config));

    // 初始化 UI
    let ui = AppWindow::new()?;
    let ui_handle = ui.as_weak();

    // 设置初始状态
    let servers: Vec<SharedString> = config
        .lock()
        .unwrap()
        .servers
        .iter()
        .map(|s| SharedString::from(&s.name))
        .collect();


    ui.set_servers(ModelRc::new(VecModel::from(servers)));

    // 设置 SSH Key 提示文案
    let ssh_hint = if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");
        format!("自动探测 (Agent 或 {})", ssh_dir.to_string_lossy())
    } else {
        "自动探测 (Agent/Default)".to_string()
    };
    ui.set_ssh_key_hint(SharedString::from(ssh_hint));

    // 如果命令行有文件参数，设置 UI
    if let Some(path_str) = &args.file {
        if let Ok(abs_path) = utils::normalize_path(path_str) {
            ui.set_file_path(SharedString::from(abs_path.to_string_lossy().to_string()));
        } else {
            ui.set_file_path(SharedString::from(path_str));
        }
    }

    // 初始化 target-dir (使用第一个服务器的默认目录)
    {
        let servers = config.lock().unwrap().servers.clone();
        if let Some(first_server) = servers.first() {
            ui.set_target_dir(SharedString::from(&first_server.default_target_dir));
        }
    }

    // 绑定文件选择
    let ui_handle_pick = ui.as_weak();
    ui.on_pick_file(move || {
        if let Some(ui) = ui_handle_pick.upgrade() {
            // 弹出文件选择框
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                ui.set_file_path(SharedString::from(path.to_string_lossy().to_string()));
            }
        }
    });

    // 绑定服务器切换 (更新默认目录)
    let config_clone_select = config.clone();
    let ui_handle_select = ui.as_weak();
    ui.on_server_selected(move |index| {
        let config = config_clone_select.lock().unwrap();
        if index >= 0 && (index as usize) < config.servers.len() {
            let server = &config.servers[index as usize];
            if let Some(ui) = ui_handle_select.upgrade() {
                ui.set_target_dir(SharedString::from(&server.default_target_dir));
            }
        }
    });

    // 绑定开始上传事件
    let config_clone = config.clone();
    let ui_handle_clone = ui_handle.clone();

    ui.on_start_upload(move |server_index| {
        let ui = ui_handle_clone.unwrap();

        // 获取当前文件路径
        let file_path_str = ui.get_file_path();
        if file_path_str == "未选择文件" {
            ui.set_status_log("请先选择文件".into());
            return;
        }

        let local_path = PathBuf::from(file_path_str.as_str());

        // 检查文件是否存在
        if let Err(e) = utils::ensure_file_exists(&local_path) {
            ui.set_status_log(format!("错误: {}", e).into());
            return;
        }

        // 获取服务器配置
        let config_guard = config_clone.lock().unwrap();
        if server_index < 0 || server_index as usize >= config_guard.servers.len() {
            ui.set_status_log("无效的服务器选择".into());
            return;
        }
        let mut server_config = config_guard.servers[server_index as usize].clone();
        drop(config_guard); // 释放锁

        // 获取 UI 上的目标目录 (允许覆盖默认配置)
        let target_dir_str = ui.get_target_dir();
        server_config.default_target_dir = target_dir_str.to_string();

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
                        }
                    }
                }
            });
        });
    });

    // 绑定保存配置事件
    let config_clone_save = config.clone();
    let ui_handle_save = ui.as_weak();
    ui.on_save_config(move |index, ui_config| {
        let mut config_guard = config_clone_save.lock().unwrap();

        let new_server = ServerConfig {
            name: ui_config.name.into(),
            host: ui_config.host.into(),
            port: ui_config.port.parse().unwrap_or(22),
            user: ui_config.user.into(),
            auth_type: ui_config.auth_type.into(),
            password: if ui_config.password.is_empty() {
                None
            } else {
                Some(ui_config.password.into())
            },
            key_path: if ui_config.key_path.is_empty() {
                None
            } else {
                Some(ui_config.key_path.into())
            },
            default_target_dir: ui_config.default_target_dir.into(),
        };

        if index == -1 {
            // 新增
            config_guard.servers.push(new_server);
        } else if index >= 0 && (index as usize) < config_guard.servers.len() {
            // 更新
            config_guard.servers[index as usize] = new_server;
        }

        if let Err(e) = config_guard.save() {
            eprintln!("Failed to save config: {}", e);
        }

        // 刷新 UI 列表
        let servers: Vec<SharedString> = config_guard
            .servers
            .iter()
            .map(|s| SharedString::from(&s.name))
            .collect();
        if let Some(ui) = ui_handle_save.upgrade() {
            ui.set_servers(ModelRc::new(VecModel::from(servers)));
            ui.set_show_settings(false); // 保存后关闭设置窗口
        }
    });

    // 绑定删除配置事件
    let config_clone_del = config.clone();
    let ui_handle_del = ui.as_weak();
    ui.on_delete_config(move |index| {
        let mut config_guard = config_clone_del.lock().unwrap();

        if index >= 0 && (index as usize) < config_guard.servers.len() {
            config_guard.servers.remove(index as usize);

            if let Err(e) = config_guard.save() {
                eprintln!("Failed to save config after delete: {}", e);
            }

            // 刷新 UI 列表
            let servers: Vec<SharedString> = config_guard
                .servers
                .iter()
                .map(|s| SharedString::from(&s.name))
                .collect();
            if let Some(ui) = ui_handle_del.upgrade() {
                ui.set_servers(ModelRc::new(VecModel::from(servers)));
                // 删除后由于索引变化，当前选中的 server 可能需要重置，或者界面逻辑会自动处理
                // 这里为了安全，重置为新建状态
                ui.set_current_settings_index(-1);
                ui.set_current_config(ServerConfigUI {
                    name: "New Server".into(),
                    host: "".into(),
                    port: "22".into(),
                    user: "root".into(),
                    auth_type: "password".into(),
                    password: "".into(),
                    key_path: "".into(),
                    default_target_dir: "/tmp".into(),
                });
            }
        }
    });

    // 绑定密钥文件选择
    let ui_handle_key = ui.as_weak();
    ui.on_pick_key_file(move || {
        if let Some(ui) = ui_handle_key.upgrade() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                // 读取当前配置
                let mut current_config = ui.get_current_config();
                current_config.key_path = SharedString::from(path.to_string_lossy().to_string());
                ui.set_current_config(current_config);
            }
        }
    });

    // 绑定加载设置配置事件
    let config_clone_load = config.clone();
    let ui_handle_load = ui.as_weak();
    ui.on_load_config(move |index| {
        let config_guard = config_clone_load.lock().unwrap();
        if index >= 0 && (index as usize) < config_guard.servers.len() {
            let server = &config_guard.servers[index as usize];
            let ui_config = ServerConfigUI {
                name: server.name.clone().into(),
                host: server.host.clone().into(),
                port: server.port.to_string().into(),
                user: server.user.clone().into(),
                auth_type: server.auth_type.clone().into(),
                password: server.password.clone().unwrap_or_default().into(),
                key_path: server.key_path.clone().unwrap_or_default().into(),
                default_target_dir: server.default_target_dir.clone().into(),
            };

            if let Some(ui) = ui_handle_load.upgrade() {
                ui.set_current_config(ui_config);
            }
        }
    });

    // 绑定测试连接事件
    let ui_handle_test = ui.as_weak();
    ui.on_test_connection(move |ui_config| {
        let server_config = ServerConfig {
            name: ui_config.name.into(),
            host: ui_config.host.into(),
            port: ui_config.port.parse().unwrap_or(22),
            user: ui_config.user.into(),
            auth_type: ui_config.auth_type.into(),
            password: if ui_config.password.is_empty() {
                None
            } else {
                Some(ui_config.password.into())
            },
            key_path: if ui_config.key_path.is_empty() {
                None
            } else {
                Some(ui_config.key_path.into())
            },
            default_target_dir: ui_config.default_target_dir.into(),
        };

        let ui_handle_test_thread = ui_handle_test.clone();
        thread::spawn(move || {
            let (result, logs) = SshUploader::connect_with_log(&server_config);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_test_thread.upgrade() {
                    ui.set_is_testing(false);
                    ui.set_test_log(logs.into()); // 设置日志内容
                    
                    match result {
                        Ok(_) => {
                            ui.set_test_success(true);
                            ui.set_test_result("成功: 连接已建立 ✅".into());
                            ui.set_show_log(false); // 成功时默认不展开日志
                        }
                        Err(e) => {
                            ui.set_test_success(false);
                            ui.set_test_result(format!("失败: {}", e).into());
                            ui.set_show_log(true); // 失败时自动展开日志
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
