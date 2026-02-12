use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::domain::config::AppConfig;
use crate::domain::ports::ConfigRepository;

pub struct TomlConfigStore;

impl TomlConfigStore {
    pub fn new() -> Self {
        Self
    }

    fn get_config_path() -> Result<PathBuf> {
        let mut path = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("无法获取系统配置目录"))?;
        path.push("flick");
        path.push("server.toml");
        Ok(path)
    }
}

impl ConfigRepository for TomlConfigStore {
    fn load(&self) -> Result<AppConfig> {
        let config_path = Self::get_config_path()?;

        if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("无法读取配置文件: {:?}", config_path))?;
            let config: AppConfig = toml::from_str(&content)
                .with_context(|| "配置文件格式错误，请检查 server.toml")?;
            Ok(config)
        } else {
            let config = AppConfig::default();
            self.save(&config)
                .with_context(|| "创建默认配置文件失败")?;
            Ok(config)
        }
    }

    fn save(&self, config: &AppConfig) -> Result<()> {
        let config_path = Self::get_config_path()?;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("无法创建配置目录: {:?}", parent))?;
        }

        let content = toml::to_string_pretty(config)
            .with_context(|| "序列化配置失败")?;

        fs::write(&config_path, content)
            .with_context(|| format!("无法写入配置文件: {:?}", config_path))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_not_empty() {
        let path = TomlConfigStore::get_config_path().unwrap();
        assert!(path.to_string_lossy().contains("flick"));
        assert!(path.to_string_lossy().contains("server.toml"));
    }
}
