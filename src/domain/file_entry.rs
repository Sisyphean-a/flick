#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FileEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}
