// 关闭控制台窗口 (仅 Release 模式且无调试输出时建议开启，此处暂保留以便调试)
// #![windows_subsystem = "windows"]

mod config;
mod local_fs;
mod remote_fs;
mod ssh_core;
mod transfer;
mod ui_bridge;
mod utils;

use clap::Parser;
use slint::{ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex};

use config::AppConfig;

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
    let config = Arc::new(Mutex::new(AppConfig::load()?));

    let ui = AppWindow::new()?;

    // 初始化服务器列表
    init_ui_state(&ui, &config, &args);

    // 绑定回调
    ui_bridge::settings::bind(&ui, config.clone());
    // Phase 4: 快速上传功能暂时禁用,将在 Phase 5 中重新实现
    // ui_bridge::quick_upload::bind(&ui, config.clone());
    ui_bridge::explorer::bind(&ui, config);

    ui.run()?;
    Ok(())
}

fn init_ui_state(
    ui: &AppWindow,
    config: &Arc<Mutex<AppConfig>>,
    _args: &Args,
) {
    let guard = config.lock().unwrap();

    // 服务器列表
    let servers: Vec<SharedString> = guard
        .servers
        .iter()
        .map(|s| SharedString::from(&s.name))
        .collect();
    ui.set_servers(ModelRc::new(VecModel::from(servers)));

    // SSH Key 提示
    let ssh_hint = match dirs::home_dir() {
        Some(home) => {
            let ssh_dir = home.join(".ssh");
            format!(
                "自动探测 (Agent 或 {})",
                ssh_dir.to_string_lossy()
            )
        }
        None => "自动探测 (Agent/Default)".to_string(),
    };
    ui.set_ssh_key_hint(SharedString::from(ssh_hint));

    // Phase 4: 以下代码暂时注释,将在 Phase 5 快速上传模式中恢复
    /*
    // 命令行文件参数
    if let Some(path_str) = &args.file {
        let display = utils::normalize_path(path_str)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path_str.clone());
        ui.set_file_path(SharedString::from(display));
    }

    // 默认目标目录
    if let Some(first) = guard.servers.first() {
        ui.set_target_dir(SharedString::from(
            &first.default_target_dir,
        ));
    }
    */
}
