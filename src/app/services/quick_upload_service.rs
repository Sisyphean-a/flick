use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::domain::config::ServerConfig;
use crate::infra::ssh::{FileTransfer, SshUploader};
use crate::shared::path_utils;

pub fn validate_upload_path(local_path: &std::path::Path) -> Result<()> {
    path_utils::ensure_file_exists(local_path)
}

pub fn execute_upload(
    config: ServerConfig,
    local_path: PathBuf,
    callback: impl Fn(f32),
) -> Result<()> {
    let mut uploader = SshUploader::connect(&config)?;

    let file_name = local_path
        .file_name()
        .ok_or_else(|| anyhow!("无效的文件名"))?;
    let remote_path = Path::new(&config.default_target_dir).join(file_name);

    uploader.upload(&local_path, &remote_path, callback)?;
    Ok(())
}
