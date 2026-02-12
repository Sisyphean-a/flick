use anyhow::{anyhow, Result};

use crate::domain::config::ServerConfig;

pub fn perform_native_ssh_check(config: &ServerConfig) -> Result<String> {
    use std::process::Command;

    let verify = Command::new("ssh").arg("-V").output();
    if verify.is_err() {
        return Err(anyhow!("系统中未找到 ssh 命令"));
    }

    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("BatchMode=yes")
       .arg("-o").arg("StrictHostKeyChecking=no")
       .arg("-p").arg(config.port.to_string())
       .arg("-T");

    if config.auth_type == "key" {
        if let Some(path) = &config.key_path {
             if !path.is_empty() {
                 cmd.arg("-i").arg(path);
             }
        }
    }

    cmd.arg(format!("{}@{}", config.user, config.host));
    cmd.arg("exit 0");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output()?;

    if output.status.success() {
        Ok("连接成功 (Exit 0)".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("Exit code {}: {}", output.status, stderr))
    }
}
