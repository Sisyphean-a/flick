use crate::config::ServerConfig;
use anyhow::{anyhow, Context, Result};
use ssh2::Session;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// 文件传输接口 (方便未来扩展 FTP/S3)
pub trait FileTransfer {
    /// 上传文件
    /// callback: 进度回调，参数为 0.0 - 1.0 的浮点数
    fn upload(
        &mut self,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()>;
}

/// SSH/SFTP 上传器
pub struct SshUploader {
    session: Session,
    _tcp: TcpStream, // 保持 TCP 连接存活
    config: ServerConfig, // 保存配置以便使用 SCP
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

        let tcp = match TcpStream::connect(format!("{}:{}", config.host, config.port)) {
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

        session.set_tcp_stream(tcp.try_clone().unwrap());
        
        if let Err(e) = session.handshake() {
            log!("SSH 握手失败: {}", e);
            return (Err(anyhow::Error::new(e).context("SSH 握手失败")), logs);
        }
        log!("SSH 握手成功");

        let auth_result = match config.auth_type.as_str() {
            "password" => {
                log!("尝试密码认证...");
                if let Some(pwd) = &config.password {
                    match session.userauth_password(&config.user, pwd) {
                        Ok(_) => {
                            log!("密码认证成功");
                            Ok(())
                        }
                        Err(e) => {
                            log!("密码认证失败: {}", e);
                            Err(anyhow!(e))
                        }
                    }
                } else {
                    log!("密码为空");
                    Err(anyhow!("密码为空"))
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
                    (Ok(Self { session, _tcp: tcp, config: config.clone() }), logs)
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
                match perform_native_ssh_check(config) {
                    Ok(msg) => {
                        log!("✅ 原生 SSH 测试成功: {}", msg);
                        log!("📌 诊断: 服务器可达,密钥有效,但 Flick 内置库不支持您的密钥格式");
                        log!("💡 当前可以正常使用文件上传功能(将使用系统 scp 命令)");
                        
                        // 返回成功状态,允许上传操作继续
                        (Ok(Self { session, _tcp: tcp, config: config.clone() }), logs)
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
}

fn perform_native_ssh_check(config: &ServerConfig) -> Result<String> {
    use std::process::Command;
    
    // 检查 ssh 是否存在
    let verify = Command::new("ssh").arg("-V").output();
    if verify.is_err() {
        return Err(anyhow!("系统中未找到 ssh 命令"));
    }

    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("BatchMode=yes") // 禁止交互输入密码
       .arg("-o").arg("StrictHostKeyChecking=no") // 忽略 Host Key 检查
       .arg("-p").arg(config.port.to_string())
       .arg("-T"); // 禁止分配伪终端

    if config.auth_type == "key" {
        if let Some(path) = &config.key_path {
             if !path.is_empty() {
                 cmd.arg("-i").arg(path);
             }
        }
    }

    cmd.arg(format!("{}@{}", config.user, config.host));
    cmd.arg("exit 0"); // 执行 exit 命令

    // Windows 下创建窗口时不显示
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


impl SshUploader {
    /// 使用 SCP 命令上传文件（支持新版 OpenSSH 密钥格式）
    fn upload_via_scp(
        config: &ServerConfig,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        use std::process::Command;
        
        callback(0.0);
        
        // 确保 scp 命令可用
        if Command::new("scp").arg("-V").output().is_err() {
            return Err(anyhow!("系统中未找到 scp 命令,请安装 OpenSSH 客户端"));
        }
        
        // 构建 scp 命令
        let mut cmd = Command::new("scp");
        cmd.arg("-P").arg(config.port.to_string())
           .arg("-o").arg("StrictHostKeyChecking=no")
           .arg("-o").arg("BatchMode=yes"); // 禁止交互式密码输入
        
        // 如果指定了密钥路径
        if let Some(key_path) = &config.key_path {
            if !key_path.is_empty() {
                cmd.arg("-i").arg(key_path);
            }
        }
        
        // Windows 下隐藏窗口
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        
        // 源文件和目标
        cmd.arg(local_path);
        cmd.arg(format!("{}@{}:{}", config.user, config.host, remote_path.to_string_lossy()));
        
        callback(0.1); // 准备完成
        
        // 执行上传
        let output = cmd.output()
            .with_context(|| "无法执行 scp 命令")?;
        
        if output.status.success() {
            callback(1.0);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!("SCP 上传失败: {}", stderr.trim()))
        }
    }

    
    /// 使用 SFTP 上传文件（原有实现，兼容旧格式密钥）
    fn upload_via_sftp(
        &mut self,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        let mut local_file = File::open(local_path)
            .with_context(|| format!("无法打开本地文件: {:?}", local_path))?;
        let metadata = local_file.metadata()?;
        let total_size = metadata.len();

        // 尝试创建远程目录
        if let Some(parent) = remote_path.parent() {
            let mut channel = self.session.channel_session()?;
            let parent_str = parent.to_string_lossy();
            let parent_unix = parent_str.replace("\\", "/");
            let _ = channel.exec(&format!("mkdir -p \"{}\"", parent_unix));
            let _ = channel.wait_close();
        }

        let sftp = self.session.sftp().with_context(|| "无法建立 SFTP 会话")?;

        let mut remote_file = sftp
            .create(remote_path)
            .with_context(|| format!("无法在远程创建文件: {:?}", remote_path))?;

        let mut buffer = [0u8; 8192];
        let mut transferred = 0u64;

        loop {
            let bytes_read = local_file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            remote_file.write_all(&buffer[..bytes_read])?;

            transferred += bytes_read as u64;
            if total_size > 0 {
                let progress = transferred as f32 / total_size as f32;
                callback(progress);
            }
        }

        callback(1.0);
        Ok(())
    }
}

impl FileTransfer for SshUploader {
    fn upload(
        &mut self,
        local_path: &Path,
        remote_path: &Path,
        callback: impl Fn(f32),
    ) -> Result<()> {
        // 优先尝试 SCP（支持新版 OpenSSH 密钥格式）
        match Self::upload_via_scp(&self.config, local_path, remote_path, &callback) {
            Ok(_) => Ok(()),
            Err(scp_err) => {
                // SCP 失败，回退到 SFTP（兼容旧格式密钥）
                self.upload_via_sftp(local_path, remote_path, callback)
                    .with_context(|| format!("SCP 和 SFTP 均失败。SCP 错误: {}", scp_err))
            }
        }
    }
}
