use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

/// 本地文件/目录条目
#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
    pub path: PathBuf,
}

/// 列出目录内容，目录优先排序
pub fn list_dir(path: &Path) -> anyhow::Result<Vec<LocalEntry>> {
    let mut entries = Vec::new();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().to_string();

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| {
                let dt: DateTime<Local> = t.into();
                Some(dt.format("%Y-%m-%d %H:%M").to_string())
            })
            .unwrap_or_default();

        entries.push(LocalEntry {
            name,
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified,
            path: entry.path(),
        });
    }

    // 目录优先，同类按名称排序
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}
