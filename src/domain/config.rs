use serde::{Deserialize, Serialize};

/// 服务器连接配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    /// 服务器别名 (显示在下拉框中)
    pub name: String,
    /// 主机地址 (IP 或域名)
    pub host: String,
    /// SSH 端口 (默认 22)
    pub port: u16,
    /// 用户名
    pub user: String,
    /// 认证方式: "password" 或 "key"
    pub auth_type: String, // "password" | "key"
    /// 密码 (如果 auth_type 为 password)
    pub password: Option<String>,
    /// 私钥路径 (如果 auth_type 为 key)
    pub key_path: Option<String>,
    /// 默认上传的目标目录
    pub default_target_dir: String,
    /// 是否为默认服务器 (启动时自动选中)
    #[serde(default)]
    pub is_default: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "本地测试服务器".to_string(),
            host: "127.0.0.1".to_string(),
            port: 22,
            user: "root".to_string(),
            auth_type: "password".to_string(),
            password: Some("123456".to_string()),
            key_path: None,
            default_target_dir: "/tmp".to_string(),
            is_default: false,
        }
    }
}

/// 书签
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bookmark {
    pub name: String,
    pub path: String,
    /// "local" 或 "remote"
    pub side: String,
}

/// 应用全局配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    /// 服务器列表
    pub servers: Vec<ServerConfig>,
    /// 上次选择的服务器索引
    pub last_selected_index: usize,
    /// 书签列表
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            servers: vec![ServerConfig::default()],
            last_selected_index: 0,
            bookmarks: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.port, 22);
        assert_eq!(cfg.auth_type, "password");
        assert!(cfg.password.is_some());
        assert!(cfg.key_path.is_none());
    }

    #[test]
    fn test_app_config_default() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.last_selected_index, 0);
    }

    #[test]
    fn test_toml_roundtrip() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.servers.len(), config.servers.len());
        assert_eq!(parsed.servers[0].host, config.servers[0].host);
        assert_eq!(parsed.servers[0].port, config.servers[0].port);
    }

}

