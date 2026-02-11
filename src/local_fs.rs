use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sysinfo::Disks;

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
    // 特殊处理：如果是空路径，列出所有盘符
    if path.as_os_str().is_empty() {
        return Ok(list_drives());
    }

    let mut entries = Vec::new();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        
        // 过滤隐藏文件和系统文件
        let attributes = metadata.file_attributes();
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        
        if (attributes & FILE_ATTRIBUTE_HIDDEN != 0) || (attributes & FILE_ATTRIBUTE_SYSTEM != 0) {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        let modified = metadata
            .modified()
            .ok()
            .map(|t| {
                let dt: DateTime<Local> = t.into();
                dt.format("%Y-%m-%d %H:%M").to_string()
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

fn list_drives() -> Vec<LocalEntry> {
    let disks = Disks::new_with_refreshed_list();
    let mut entries = Vec::new();

    for disk in disks.list() {
        let mount_point = disk.mount_point();

        // 过滤掉非根目录的挂载点（主要针对 Windows 上的 Docker 容器卷等）
        // 如果有父目录，说明它不是一个根驱动器（如 C:\）
        if mount_point.parent().is_some() {
            continue;
        }

        entries.push(LocalEntry {
            name: mount_point.to_string_lossy().to_string(),
            is_dir: true,
            size: disk.total_space(),
            modified: String::new(),
            path: mount_point.to_path_buf(),
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::process::Command;

    #[test]
    fn test_list_drives() {
        // 确保能列出至少一个盘符 (CI 环境可能不同，但在 Windows 开发机上通常有 C:)
        let drives = list_drives();
        assert!(!drives.is_empty(), "应该能列出至少一个盘符");
        
        for drive in drives {
            println!("Found drive: {:?}", drive.name);
            assert!(drive.is_dir);
        }
    }

    #[test]
    fn test_filter_hidden_files() {
        use std::env;
        let temp_dir = env::temp_dir().join("flick_test_hidden");
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).unwrap();
        }
        fs::create_dir(&temp_dir).unwrap();

        let normal_file = temp_dir.join("normal.txt");
        File::create(&normal_file).unwrap();

        let hidden_file = temp_dir.join("hidden.txt");
        File::create(&hidden_file).unwrap();

        // Set hidden attribute on Windows
        Command::new("attrib")
            .arg("+h")
            .arg(&hidden_file)
            .status()
            .expect("failed to execute attrib");

        let entries = list_dir(&temp_dir).unwrap();
        
        let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"normal.txt".to_string()));
        assert!(!names.contains(&"hidden.txt".to_string()));

        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
