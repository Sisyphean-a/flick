use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::domain::config::AppConfig;
use crate::domain::ports::ConfigRepository;
use crate::infra::config_store::TomlConfigStore;

pub struct AppContext {
    pub config: Arc<Mutex<AppConfig>>,
    pub config_repo: Arc<dyn ConfigRepository + Send + Sync>,
}

impl AppContext {
    pub fn bootstrap() -> Result<Self> {
        let repo = Arc::new(TomlConfigStore::new());
        let config = repo.load()?;
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            config_repo: repo,
        })
    }
}
