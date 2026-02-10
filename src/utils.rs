use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 标准化路径，处理相对路径和波浪号 (~) 等
/// 目前简单实现，直接返回绝对路径
/// 标准化路径
#[allow(dead_code)]
pub fn normalize_path(path_str: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_str);

    // 尝试直接获取绝对路径 (解析 . .. 等)
    if let Ok(p) = std::fs::canonicalize(&path) {
        return Ok(p);
    }

    // 如果文件不存在，手动拼接
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .with_context(|| "无法获取当前工作目录")
    }
}

/// 检查文件是否存在且是文件
#[allow(dead_code)]
pub fn ensure_file_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("文件不存在: {:?}", path);
    }
    if !path.is_file() {
        anyhow::bail!("路径不是一个文件: {:?}", path);
    }
    Ok(())
}
