use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 标准化路径，处理相对路径和波浪号 (~) 等
/// 目前简单实现，直接返回绝对路径
/// 标准化路径
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
pub fn ensure_file_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("文件不存在: {:?}", path);
    }
    if !path.is_file() {
        anyhow::bail!("路径不是一个文件: {:?}", path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_normalize_absolute_path() {
        let result = normalize_path("/tmp/test.txt").unwrap();
        assert!(result.is_absolute());
    }

    #[test]
    fn test_normalize_relative_path() {
        let result = normalize_path("some_file.txt").unwrap();
        assert!(result.is_absolute());
    }

    #[test]
    fn test_ensure_file_exists_missing() {
        let result = ensure_file_exists(Path::new("/nonexistent_file_12345.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_file_exists_is_dir() {
        let dir = std::env::temp_dir();
        let result = ensure_file_exists(&dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_file_exists_ok() {
        let tmp = std::env::temp_dir().join("flick_test_utils.tmp");
        fs::write(&tmp, "test").unwrap();
        let result = ensure_file_exists(&tmp);
        assert!(result.is_ok());
        let _ = fs::remove_file(&tmp);
    }
}
