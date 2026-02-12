use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};

use crate::domain::config::{AppConfig, ServerConfig};
use crate::domain::ports::ConfigRepository;
use crate::infra::ssh::SshUploader;

pub fn save_server(
    config_state: &Arc<Mutex<AppConfig>>,
    repo: &Arc<dyn ConfigRepository + Send + Sync>,
    index: i32,
    new_server: ServerConfig,
) -> Result<AppConfig> {
    let mut guard = config_state
        .lock()
        .map_err(|_| anyhow!("配置锁定失败"))?;

    if index == -1 {
        if new_server.is_default {
            for s in &mut guard.servers {
                s.is_default = false;
            }
        }
        guard.servers.push(new_server);
    } else if index >= 0 && (index as usize) < guard.servers.len() {
        if new_server.is_default {
            for s in &mut guard.servers {
                s.is_default = false;
            }
        }
        guard.servers[index as usize] = new_server;
    }

    repo.save(&guard)?;
    Ok(guard.clone())
}

pub fn delete_server(
    config_state: &Arc<Mutex<AppConfig>>,
    repo: &Arc<dyn ConfigRepository + Send + Sync>,
    index: i32,
) -> Result<AppConfig> {
    let mut guard = config_state
        .lock()
        .map_err(|_| anyhow!("配置锁定失败"))?;

    if index >= 0 && (index as usize) < guard.servers.len() {
        guard.servers.remove(index as usize);
        repo.save(&guard)?;
    }

    Ok(guard.clone())
}

pub fn load_server(
    config_state: &Arc<Mutex<AppConfig>>,
    index: i32,
) -> Option<ServerConfig> {
    let guard = config_state.lock().ok()?;
    if index >= 0 && (index as usize) < guard.servers.len() {
        Some(guard.servers[index as usize].clone())
    } else {
        None
    }
}

pub fn test_connection(server_config: &ServerConfig) -> (Result<()>, String) {
    let (res, logs) = SshUploader::connect_with_log(server_config);
    (res.map(|_| ()), logs)
}
