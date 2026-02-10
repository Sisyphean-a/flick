use crate::config::ServerConfig;
use crate::ssh_core::{AuthMode, SshUploader};
use anyhow::{anyhow, Result};
use chrono::{Local, TimeZone};
use std::path::Path;

/// 远程文件/目录条目
#[derive(Debug, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

/// 通过 SFTP 列出远程目录
pub fn list_dir_sftp(
    uploader: &SshUploader,
    path: &str,
) -> Result<Vec<RemoteEntry>> {
    if *uploader.auth_mode() == AuthMode::NativeSsh {
        return list_dir_native(uploader.config(), path);
    }

    let sftp = uploader
        .session()
        .sftp()
        .map_err(|e| anyhow!("SFTP 会话失败: {}", e))?;

    let dir = sftp
        .readdir(Path::new(path))
        .map_err(|e| anyhow!("读取目录失败: {}", e))?;

    let mut entries: Vec<RemoteEntry> = dir
        .into_iter()
        .filter_map(|(p, stat)| {
            let name = p.file_name()?.to_string_lossy().to_string();
            if name == "." || name == ".." {
                return None;
            }
            let is_dir = stat.is_dir();
            let size = stat.size.unwrap_or(0);
            let modified = stat
                .mtime
                .and_then(|t| {
                    Local.timestamp_opt(t as i64, 0).single()
                })
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();

            Some(RemoteEntry {
                name,
                is_dir,
                size,
                modified,
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

/// 通过系统 ssh 命令列出远程目录（NativeSsh 兜底）
fn list_dir_native(
    config: &ServerConfig,
    path: &str,
) -> Result<Vec<RemoteEntry>> {
    use std::process::Command;

    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("BatchMode=yes")
        .arg("-o").arg("StrictHostKeyChecking=no")
        .arg("-p").arg(config.port.to_string());

    if let Some(key) = &config.key_path {
        if !key.is_empty() {
            cmd.arg("-i").arg(key);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    cmd.arg(format!("{}@{}", config.user, config.host));
    cmd.arg(format!("ls -la --time-style=long-iso \"{}\"", path));

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ls 失败: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = parse_ls_output(&stdout);

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

/// 解析 `ls -la --time-style=long-iso` 输出
fn parse_ls_output(output: &str) -> Vec<RemoteEntry> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> =
                line.split_whitespace().collect();
            if parts.len() < 8 {
                return None;
            }
            let perms = parts[0];
            if perms == "total" {
                return None;
            }
            let is_dir = perms.starts_with('d');
            let size: u64 = parts[4].parse().unwrap_or(0);
            let date = parts[5];
            let time = parts[6];
            let name = parts[7..].join(" ");
            if name == "." || name == ".." {
                return None;
            }
            Some(RemoteEntry {
                name,
                is_dir,
                size,
                modified: format!("{} {}", date, time),
            })
        })
        .collect()
}
