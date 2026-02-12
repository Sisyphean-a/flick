use anyhow::{anyhow, Context, Result};
use ssh2::Session;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use crate::domain::config::ServerConfig;
use crate::infra::remote_fs;

use super::{FileTransfer, SshUploader};

pub fn ensure_scp_available() -> Result<()> {
    use std::process::Command;

    if Command::new("scp").arg("-V").output().is_err() {
        anyhow::bail!("系统中未找到 scp 命令,请安装 OpenSSH 客户端")
    }
    Ok(())
}

pub fn build_remote_target(config: &ServerConfig, remote_path: &Path) -> String {
    format!("{}@{}:{}", config.user, config.host, remote_path.to_string_lossy())
}

fn upload_via_scp(
    config: &ServerConfig,
    local_path: &Path,
    remote_path: &Path,
    callback: impl Fn(f32),
) -> Result<()> {
    use std::process::Command;

    callback(0.0);
    ensure_scp_available()?;

    let mut cmd = Command::new("scp");
    cmd.arg("-P")
        .arg(config.port.to_string())
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("BatchMode=yes");

    if let Some(key_path) = &config.key_path {
        if !key_path.is_empty() {
            cmd.arg("-i").arg(key_path);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.arg(local_path);
    cmd.arg(build_remote_target(config, remote_path));

    callback(0.1);

    let output = cmd.output().with_context(|| "无法执行 scp 命令")?;

    if output.status.success() {
        callback(1.0);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("SCP 上传失败: {}", stderr.trim()))
    }
}

fn upload_via_sftp(
    session: &Session,
    local_path: &Path,
    remote_path: &Path,
    callback: impl Fn(f32),
) -> Result<()> {
    let mut local_file = File::open(local_path)
        .with_context(|| format!("无法打开本地文件: {:?}", local_path))?;
    let metadata = local_file.metadata()?;
    let total_size = metadata.len();

    if let Some(parent) = remote_path.parent() {
        let mut channel = session.channel_session()?;
        let parent_str = parent.to_string_lossy();
        let parent_unix = parent_str.replace("\\", "/");
        let _ = channel.exec(&format!("mkdir -p \"{}\"", parent_unix));
        let _ = channel.wait_close();
    }

    let sftp = session.sftp().with_context(|| "无法建立 SFTP 会话")?;

    let mut remote_file = sftp
        .create(remote_path)
        .with_context(|| format!("无法在远程创建文件: {:?}", remote_path))?;

    let mut buffer = [0u8; 8192];
    let mut transferred = 0u64;

    loop {
        let bytes_read = local_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        remote_file.write_all(&buffer[..bytes_read])?;

        transferred += bytes_read as u64;
        if total_size > 0 {
            let progress = transferred as f32 / total_size as f32;
            callback(progress);
        }
    }

    callback(1.0);
    Ok(())
}

fn download_via_sftp(
    session: &Session,
    remote_path: &Path,
    local_path: &Path,
    callback: impl Fn(f32),
) -> Result<()> {
    let sftp = session.sftp().with_context(|| "无法建立 SFTP 会话")?;

    let mut remote_file = sftp
        .open(remote_path)
        .with_context(|| format!("无法打开远程文件: {:?}", remote_path))?;

    let stat = remote_file.stat()?;
    let total_size = stat.size.unwrap_or(0);

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建本地目录: {:?}", parent))?;
    }

    let mut local_file = File::create(local_path)
        .with_context(|| format!("无法创建本地文件: {:?}", local_path))?;

    let mut buffer = [0u8; 8192];
    let mut transferred = 0u64;

    loop {
        let bytes_read = remote_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        local_file.write_all(&buffer[..bytes_read])?;

        transferred += bytes_read as u64;
        if total_size > 0 {
            let progress = transferred as f32 / total_size as f32;
            callback(progress);
        }
    }

    callback(1.0);
    Ok(())
}

fn download_via_scp(
    config: &ServerConfig,
    remote_path: &Path,
    local_path: &Path,
    callback: impl Fn(f32),
) -> Result<()> {
    use std::process::Command;

    callback(0.0);
    ensure_scp_available()?;

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建本地目录: {:?}", parent))?;
    }

    let mut cmd = Command::new("scp");
    cmd.arg("-P")
        .arg(config.port.to_string())
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("BatchMode=yes");

    if let Some(key_path) = &config.key_path {
        if !key_path.is_empty() {
            cmd.arg("-i").arg(key_path);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.arg(build_remote_target(config, remote_path));
    cmd.arg(local_path);

    callback(0.1);

    let output = cmd.output().with_context(|| "无法执行 scp 命令")?;

    if output.status.success() {
        callback(1.0);
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("SCP 下载失败: {}", stderr.trim()))
    }
}

fn upload_dir_recursive(
    uploader: &mut SshUploader,
    local_dir: &Path,
    remote_dir: &Path,
    callback: &dyn Fn(f32),
) -> Result<()> {
    uploader.remote_mkdir(remote_dir)?;

    let entries: Vec<_> = std::fs::read_dir(local_dir)
        .with_context(|| format!("无法读取本地目录: {:?}", local_dir))?
        .filter_map(|e| e.ok())
        .collect();

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let path = entry.path();
        let name = entry.file_name();
        let remote_child = remote_dir.join(&name);

        if path.is_dir() {
            upload_dir_recursive(uploader, &path, &remote_child, callback)?;
        } else {
            uploader.upload(&path, &remote_child, callback)?;
        }

        if total > 0 {
            callback((i + 1) as f32 / total as f32);
        }
    }
    Ok(())
}

fn download_dir_recursive(
    uploader: &mut SshUploader,
    remote_dir: &Path,
    local_dir: &Path,
    callback: &dyn Fn(f32),
) -> Result<()> {
    std::fs::create_dir_all(local_dir)
        .with_context(|| format!("无法创建本地目录: {:?}", local_dir))?;

    let remote_str = remote_dir.to_string_lossy().replace('\\', "/");
    let entries = remote_fs::list_dir_sftp(uploader, &remote_str)?;

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let remote_child = remote_dir.join(&entry.name);
        let local_child = local_dir.join(&entry.name);

        if entry.is_dir {
            download_dir_recursive(
                uploader,
                &remote_child,
                &local_child,
                callback,
            )?;
        } else {
            uploader.download(&remote_child, &local_child, callback)?;
        }

        if total > 0 {
            callback((i + 1) as f32 / total as f32);
        }
    }
    Ok(())
}

impl FileTransfer for SshUploader {
    fn upload(
        &mut self,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        match upload_via_scp(self.config(), local_path, remote_path, &callback)
        {
            Ok(_) => Ok(()),
            Err(scp_err) => upload_via_sftp(
                self.session(),
                local_path,
                remote_path,
                callback,
            )
            .with_context(|| format!("SCP 和 SFTP 均失败。SCP 错误: {}", scp_err)),
        }
    }

    fn download(
        &mut self,
        remote_path: &Path,
        local_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        match download_via_scp(
            self.config(),
            remote_path,
            local_path,
            &callback,
        ) {
            Ok(_) => Ok(()),
            Err(scp_err) => download_via_sftp(
                self.session(),
                remote_path,
                local_path,
                callback,
            )
            .with_context(|| format!("SCP 和 SFTP 均失败。SCP 错误: {}", scp_err)),
        }
    }

    fn upload_dir(
        &mut self,
        local_dir: &Path,
        remote_dir: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        upload_dir_recursive(self, local_dir, remote_dir, &callback)
    }

    fn download_dir(
        &mut self,
        remote_dir: &Path,
        local_dir: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        download_dir_recursive(self, remote_dir, local_dir, &callback)
    }
}
