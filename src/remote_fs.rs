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

/// 转义 shell 参数，防止注入攻击
/// 用单引号包裹，内部单引号用 '\'' 转义
pub fn escape_shell_arg(arg: &str) -> String {
    let escaped = arg.replace('\'', "'\\''");
    format!("'{}'", escaped)
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
    cmd.arg(format!("ls -la --time-style=long-iso {}", escape_shell_arg(path)));

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

/// 在远程执行 shell 命令的辅助函数
fn remote_exec(uploader: &SshUploader, command: &str) -> Result<String> {
    if *uploader.auth_mode() == AuthMode::NativeSsh {
        return remote_exec_native(uploader.config(), command);
    }
    let mut channel = uploader.session().channel_session()
        .map_err(|e| anyhow!("创建 channel 失败: {}", e))?;
    channel.exec(command).map_err(|e| anyhow!("执行命令失败: {}", e))?;
    let mut output = String::new();
    std::io::Read::read_to_string(&mut channel, &mut output)?;
    channel.wait_close().ok();
    let exit = channel.exit_status().unwrap_or(-1);
    if exit != 0 {
        return Err(anyhow!("命令退出码 {}: {}", exit, output.trim()));
    }
    Ok(output)
}

fn remote_exec_native(config: &ServerConfig, command: &str) -> Result<String> {
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
    cmd.arg(command);
    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("命令失败: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 在远程创建目录
pub fn remote_mkdir(uploader: &SshUploader, path: &str) -> Result<()> {
    let cmd = format!("mkdir -p {}", escape_shell_arg(path));
    remote_exec(uploader, &cmd)?;
    Ok(())
}

/// 删除远程文件或目录
pub fn remote_remove(uploader: &SshUploader, path: &str, is_dir: bool) -> Result<()> {
    let cmd = if is_dir {
        format!("rm -rf {}", escape_shell_arg(path))
    } else {
        format!("rm -f {}", escape_shell_arg(path))
    };
    remote_exec(uploader, &cmd)?;
    Ok(())
}

/// 重命名远程文件或目录
pub fn remote_rename(uploader: &SshUploader, old_path: &str, new_path: &str) -> Result<()> {
    let cmd = format!("mv {} {}", escape_shell_arg(old_path), escape_shell_arg(new_path));
    remote_exec(uploader, &cmd)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_shell_arg_simple() {
        assert_eq!(escape_shell_arg("/tmp/test"), "'/tmp/test'");
    }

    #[test]
    fn test_escape_shell_arg_with_single_quote() {
        assert_eq!(escape_shell_arg("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_escape_shell_arg_with_spaces() {
        assert_eq!(escape_shell_arg("/path/with spaces"), "'/path/with spaces'");
    }

    #[test]
    fn test_escape_shell_arg_injection() {
        let malicious = "/tmp; rm -rf /";
        let escaped = escape_shell_arg(malicious);
        assert_eq!(escaped, "'/tmp; rm -rf /'");
    }

    #[test]
    fn test_parse_ls_output_basic() {
        let output = "total 8\n\
            drwxr-xr-x 2 root root 4096 2024-01-15 10:30 subdir\n\
            -rw-r--r-- 1 root root 1234 2024-01-15 09:00 file.txt\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "subdir");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, 1234);
    }

    #[test]
    fn test_parse_ls_output_skips_dots() {
        let output = "total 4\n\
            drwxr-xr-x 2 root root 4096 2024-01-15 10:30 .\n\
            drwxr-xr-x 3 root root 4096 2024-01-15 10:30 ..\n\
            -rw-r--r-- 1 root root  100 2024-01-15 09:00 readme.md\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "readme.md");
    }

    #[test]
    fn test_parse_ls_output_empty() {
        let entries = parse_ls_output("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_ls_output_filename_with_spaces() {
        let output = "-rw-r--r-- 1 root root 500 2024-01-15 09:00 my file name.txt\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my file name.txt");
    }
}
