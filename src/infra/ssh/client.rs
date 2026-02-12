use crate::domain::config::ServerConfig;
use anyhow::{anyhow, Result};
use ssh2::Session;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use super::{auth, native_fallback};

/// 文件传输接口 (方便未来扩展 FTP/S3)
pub trait FileTransfer {
    /// 上传文件
    fn upload(
        &mut self,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()>;

    /// 下载文件
    fn download(
        &mut self,
        remote_path: &Path,
        local_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()>;

    /// 上传目录（递归）
    fn upload_dir(
        &mut self,
        local_dir: &Path,
        remote_dir: &Path,
        callback: impl Fn(f32),
    ) -> Result<()>;

    /// 下载目录（递归）
    fn download_dir(
        &mut self,
        remote_dir: &Path,
        local_dir: &Path,
        callback: impl Fn(f32),
    ) -> Result<()>;
}

/// 认证模式标记
#[derive(Debug, Clone, PartialEq)]
pub enum AuthMode {
    /// libssh2 原生认证成功，session 可用
    LibSsh2,
    /// libssh2 失败，回退到系统 ssh/scp 命令
    NativeSsh,
}

/// SSH/SFTP 上传器
pub struct SshUploader {
    session: Session,
    _tcp: TcpStream, // 保持 TCP 连接存活
    config: ServerConfig, // 保存配置以便使用 SCP
    auth_mode: AuthMode,
}


impl SshUploader {
    /// 建立 SSH 连接 (带日志)
    pub fn connect_with_log(config: &ServerConfig) -> (Result<Self>, String) {
        let mut logs = String::new();
        
        macro_rules! log {
            ($($arg:tt)*) => {
                let _ = std::fmt::write(&mut logs, format_args!($($arg)*));
                logs.push('\n');
            };
        }

        log!("开始连接到 {}:{} (User: {})...", config.host, config.port, config.user);

        let tcp = match format!("{}:{}", config.host, config.port)
            .to_socket_addrs()
            .and_then(|mut addrs| {
                addrs
                    .next()
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "无法解析地址"))
                    .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(10)))
            }) {
            Ok(s) => {
                log!("TCP 连接成功");
                s
            }
            Err(e) => {
                log!("TCP 连接失败: {}", e);
                return (Err(anyhow::Error::new(e).context("TCP 连接失败")), logs);
            }
        };

        let mut session = match Session::new() {
            Ok(s) => s,
            Err(e) => {
                log!("Session 创建失败: {}", e);
                return (Err(anyhow::Error::new(e).context("Session 创建失败")), logs);
            }
        };

        let tcp_clone = match tcp.try_clone() {
            Ok(c) => c,
            Err(e) => {
                log!("TCP 克隆失败: {}", e);
                return (Err(anyhow::Error::new(e).context("TCP 克隆失败")), logs);
            }
        };
        session.set_tcp_stream(tcp_clone);
        session.set_timeout(30_000);
        
        if let Err(e) = session.handshake() {
            log!("SSH 握手失败: {}", e);
            return (Err(anyhow::Error::new(e).context("SSH 握手失败")), logs);
        }
        log!("SSH 握手成功");

        let auth_result = match config.auth_type.as_str() {
            "password" => {
                log!("尝试密码认证...");
                match auth::try_auth_with_password(&session, config) {
                    Ok(_) => {
                        log!("密码认证成功");
                        Ok(())
                    }
                    Err(e) => {
                        log!("密码认证失败: {}", e);
                        Err(e)
                    }
                }
            }
            "key" => {
                log!("尝试密钥认证...");
                let mut authenticated = false;
                
                // 1. 显式指定
                if let Some(path_str) = &config.key_path {
                    if !path_str.is_empty() {
                        log!("尝试指定密钥: {}", path_str);
                        match session.userauth_pubkey_file(&config.user, None, Path::new(path_str), None) {
                            Ok(_) => {
                                log!("指定密钥认证成功");
                                authenticated = true;
                            }
                            Err(e) => {
                                log!("指定密钥认证失败: {}", e);
                            }
                        }
                    }
                }

                if !authenticated {
                    log!("尝试 SSH Agent...");
                    match session.userauth_agent(&config.user) {
                        Ok(_) => {
                            log!("SSH Agent 认证成功");
                             authenticated = true;
                        }
                        Err(e) => {
                            log!("SSH Agent 认证失败/跳过: {}", e); 
                        }
                    }
                }

                if !authenticated {
                    log!("尝试自动探测 .ssh 目录...");
                    if let Some(home) = dirs::home_dir() {
                        let ssh_dir = home.join(".ssh");
                        if let Ok(entries) = std::fs::read_dir(&ssh_dir) {
                             for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() { continue; }
                                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                                    if file_name.ends_with(".pub") 
                                        || file_name == "known_hosts" || file_name.starts_with("known_hosts")
                                        || file_name == "config" 
                                        || file_name == "authorized_keys" {
                                        continue;
                                    }
                                    
                                    log!("尝试密钥文件: {:?}", file_name);
                                    match session.userauth_pubkey_file(&config.user, None, &path, None) {
                                        Ok(_) => {
                                            log!("认证成功!");
                                            authenticated = true;
                                            break;
                                        }
                                        Err(e) => {
                                            log!("密钥文件不匹配: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        log!("无法获取用户主目录");
                    }
                }

                if authenticated {
                    Ok(())
                } else {
                    log!("所有密钥尝试均失败");
                    Err(anyhow!("密钥认证全数失败"))
                }
            }
            _ => Err(anyhow!("不支持的认证类型")),
        };

        match auth_result {
            Ok(_) => {
                if session.authenticated() {
                    log!("最终认证状态: 已连接");
                    (Ok(Self { session, _tcp: tcp, config: config.clone(), auth_mode: AuthMode::LibSsh2 }), logs)
                } else {
                    log!("Session 标记为未认证");
                    (Err(anyhow!("认证未通过")), logs)
                }
            }
            Err(e) => {
                log!("认证流程结束: {}", e);
                
                // 尝试 Native SSH 兜底
                log!("--------------------------------");
                log!("尝试系统原生 SSH 命令...");
                match native_fallback::perform_native_ssh_check(config) {
                    Ok(msg) => {
                        log!("✅ 原生 SSH 测试成功: {}", msg);
                        log!("📌 诊断: 服务器可达,密钥有效,但 Flick 内置库不支持您的密钥格式");
                        log!("💡 当前可以正常使用文件上传功能(将使用系统 scp 命令)");
                        
                        // 返回成功状态,允许上传操作继续
                        (Ok(Self { session, _tcp: tcp, config: config.clone(), auth_mode: AuthMode::NativeSsh }), logs)
                    }
                    Err(nt_e) => {
                        log!("❌ 原生 SSH 也失败: {}", nt_e);
                        log!("诊断: 网络不通或配置错误,请检查 IP、端口、用户名");
                        (Err(e), logs)
                    }
                }

            }
        }
    }
    
    /// 兼容旧接口
    pub fn connect(config: &ServerConfig) -> Result<Self> {
        let (res, _) = Self::connect_with_log(config);
        res
    }

    /// 获取 SSH session 引用（仅 LibSsh2 模式下有效）
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// 获取认证模式
    pub fn auth_mode(&self) -> &AuthMode {
        &self.auth_mode
    }

    /// 获取服务器配置
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// 在远程创建目录（递归）
    pub fn remote_mkdir(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().replace('\\', "/");
        if self.auth_mode == AuthMode::LibSsh2 {
            let mut channel = self.session.channel_session()
                .map_err(|e| anyhow!("创建 channel 失败: {}", e))?;
            let _ = channel.exec(&format!("mkdir -p '{}'", path_str.replace('\'', "'\\''")));
            let _ = channel.wait_close();
        } else {
            use std::process::Command;
            let mut cmd = Command::new("ssh");
            cmd.arg("-o").arg("BatchMode=yes")
                .arg("-o").arg("StrictHostKeyChecking=no")
                .arg("-p").arg(self.config.port.to_string());
            if let Some(key) = &self.config.key_path {
                if !key.is_empty() {
                    cmd.arg("-i").arg(key);
                }
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            cmd.arg(format!("{}@{}", self.config.user, self.config.host));
            cmd.arg(format!("mkdir -p '{}'", path_str.replace('\'', "'\\''")));
            let _ = cmd.output();
        }
        Ok(())
    }
}



