use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::app::services::settings_service;
use crate::domain::config::AppConfig;
use crate::domain::ports::ConfigRepository;
use crate::presentation::slint::mapper;
use crate::AppWindow;

pub fn bind(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    repo: Arc<dyn ConfigRepository + Send + Sync>,
) {
    bind_save(ui, config.clone(), repo.clone());
    bind_delete(ui, config.clone(), repo);
    bind_load(ui, config);
    bind_pick_key(ui);
    bind_test(ui);
}

fn refresh_server_list(ui: &AppWindow, config: &AppConfig) {
    let servers: Vec<SharedString> = config
        .servers
        .iter()
        .map(|s| SharedString::from(&s.name))
        .collect();
    ui.set_servers(ModelRc::new(VecModel::from(servers)));
}

fn bind_save(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    repo: Arc<dyn ConfigRepository + Send + Sync>,
) {
    let ui_handle = ui.as_weak();
    ui.on_save_config(move |index, ui_config| {
        let new_server = mapper::from_ui(&ui_config);
        let updated = match settings_service::save_server(
            &config,
            &repo,
            index,
            new_server,
        ) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Failed to save config: {}", e);
                return;
            }
        };

        if let Some(ui) = ui_handle.upgrade() {
            refresh_server_list(&ui, &updated);
            ui.set_show_settings(false);
        }
    });
}

fn bind_delete(
    ui: &AppWindow,
    config: Arc<Mutex<AppConfig>>,
    repo: Arc<dyn ConfigRepository + Send + Sync>,
) {
    let ui_handle = ui.as_weak();
    ui.on_delete_config(move |index| {
        let updated = match settings_service::delete_server(
            &config,
            &repo,
            index,
        ) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Failed to save config after delete: {}", e);
                return;
            }
        };
        if let Some(ui) = ui_handle.upgrade() {
            refresh_server_list(&ui, &updated);
            ui.set_current_settings_index(-1);
            ui.set_current_config(mapper::default_ui_config());
        }
    });
}

fn bind_load(ui: &AppWindow, config: Arc<Mutex<AppConfig>>) {
    let ui_handle = ui.as_weak();
    ui.on_load_config(move |index| {
        if let Some(server) = settings_service::load_server(&config, index)
        {
            let ui_config = mapper::to_ui(&server);
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_current_config(ui_config);
            }
        }
    });
}

fn bind_pick_key(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_pick_key_file(move || {
        if let Some(ui) = ui_handle.upgrade() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                let mut cfg = ui.get_current_config();
                cfg.key_path = SharedString::from(
                    path.to_string_lossy().as_ref(),
                );
                ui.set_current_config(cfg);
            }
        }
    });
}

fn bind_test(ui: &AppWindow) {
    let ui_handle = ui.as_weak();
    ui.on_test_connection(move |ui_config| {
        let server_config = mapper::from_ui(&ui_config);
        let ui_handle_thread = ui_handle.clone();

        thread::spawn(move || {
            let (result, logs) =
                settings_service::test_connection(&server_config);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_thread.upgrade() {
                    ui.set_is_testing(false);
                    ui.set_test_log(logs.into());
                    match result {
                        Ok(_) => {
                            ui.set_test_success(true);
                            ui.set_test_result("成功: 连接已建立 ✅".into());
                            ui.set_show_log(false);
                        }
                        Err(e) => {
                            ui.set_test_success(false);
                            ui.set_test_result(
                                format!("失败: {}", e).into(),
                            );
                            ui.set_show_log(true);
                        }
                    }
                }
            });
        });
    });
}

