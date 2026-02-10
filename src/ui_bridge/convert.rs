use slint::SharedString;

use crate::config::ServerConfig;
use crate::ServerConfigUI;

/// ServerConfig -> ServerConfigUI
pub fn to_ui(server: &ServerConfig) -> ServerConfigUI {
    ServerConfigUI {
        name: SharedString::from(&server.name),
        host: SharedString::from(&server.host),
        port: SharedString::from(server.port.to_string()),
        user: SharedString::from(&server.user),
        auth_type: SharedString::from(&server.auth_type),
        password: SharedString::from(server.password.as_deref().unwrap_or("")),
        key_path: SharedString::from(server.key_path.as_deref().unwrap_or("")),
        default_target_dir: SharedString::from(&server.default_target_dir),
    }
}

/// ServerConfigUI -> ServerConfig
pub fn from_ui(ui_config: &ServerConfigUI) -> ServerConfig {
    ServerConfig {
        name: ui_config.name.to_string(),
        host: ui_config.host.to_string(),
        port: ui_config.port.as_str().parse::<u16>()
            .ok()
            .filter(|&p| p >= 1)
            .unwrap_or(22),
        user: ui_config.user.to_string(),
        auth_type: ui_config.auth_type.to_string(),
        password: if ui_config.password.is_empty() {
            None
        } else {
            Some(ui_config.password.to_string())
        },
        key_path: if ui_config.key_path.is_empty() {
            None
        } else {
            Some(ui_config.key_path.to_string())
        },
        default_target_dir: ui_config.default_target_dir.to_string(),
    }
}

/// 默认的 UI 配置（新建服务器时使用）
pub fn default_ui_config() -> ServerConfigUI {
    ServerConfigUI {
        name: "New Server".into(),
        host: "".into(),
        port: "22".into(),
        user: "root".into(),
        auth_type: "password".into(),
        password: "".into(),
        key_path: "".into(),
        default_target_dir: "/tmp".into(),
    }
}
