use anyhow::Result;

use super::config::AppConfig;

pub trait ConfigRepository {
    fn load(&self) -> Result<AppConfig>;
    fn save(&self, config: &AppConfig) -> Result<()>;
}
