use anyhow::Result;
use ssh2::Session;

use crate::domain::config::ServerConfig;

pub fn try_auth_with_password(
    session: &Session,
    config: &ServerConfig,
) -> Result<()> {
    if let Some(pwd) = &config.password {
        session.userauth_password(&config.user, pwd)?;
        Ok(())
    } else {
        anyhow::bail!("密码为空")
    }
}
