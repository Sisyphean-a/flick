use ssh2::Session;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use crate::config::ServerConfig;

/// 文件传输接口 (方便未来扩展 FTP/S3)
pub trait FileTransfer {
    /// 上传文件
    /// callback: 进度回调，参数为 0.0 - 1.0 的浮点数
    fn upload(&mut self, local_path: &Path, remote_path: &Path, callback: impl Fn(f32)) -> Result<()>;
}

/// SSH/SFTP 上传器
pub struct SshUploader {
    session: Session,
    _tcp: TcpStream, // 保持 TCP 连接存活
}

impl SshUploader {
    /// 建立 SSH 连接
    pub fn connect(config: &ServerConfig) -> Result<Self> {
        let tcp = TcpStream::connect(format!("{}:{}", config.host, config.port))
            .with_context(|| format!("无法连接到服务器 {}:{}", config.host, config.port))?;
        
        let mut session = Session::new().unwrap();
        session.set_tcp_stream(tcp.try_clone().unwrap());
        session.handshake().with_context(|| "SSH 握手失败")?;

        match config.auth_type.as_str() {
            "password" => {
                if let Some(pwd) = &config.password {
                    session.userauth_password(&config.user, pwd)
                        .with_context(|| "密码认证失败")?;
                } else {
                    return Err(anyhow!("配置为密码认证，但密码为空"));
                }
            }
            "key" => {
                if let Some(key_path) = &config.key_path {
                    session.userauth_pubkey_file(
                        &config.user,
                        None,
                        Path::new(key_path),
                        None, // passphrase unsupported for simplicity now
                    ).with_context(|| format!("密钥认证失败: {}", key_path))?;
                } else {
                    return Err(anyhow!("配置为密钥认证，但密钥路径为空"));
                }
            }
            _ => return Err(anyhow!("不支持的认证类型: {}", config.auth_type)),
        }

        if !session.authenticated() {
            return Err(anyhow!("认证失败 (未知原因)"));
        }

        Ok(Self { session, _tcp: tcp })
    }
}

impl FileTransfer for SshUploader {
    fn upload(&mut self, local_path: &Path, remote_path: &Path, callback: impl Fn(f32)) -> Result<()> {
        let mut local_file = File::open(local_path)
            .with_context(|| format!("无法打开本地文件: {:?}", local_path))?;
        let metadata = local_file.metadata()?;
        let total_size = metadata.len();

        let sftp = self.session.sftp()
            .with_context(|| "无法建立 SFTP 会话")?;
        
        // 确保远程目录存在是比较麻烦的，这里假设通过 absolute path 直接写入
        // 如果需要 mkdir -p，需要额外实现
        
        let mut remote_file = sftp.create(remote_path)
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
        
        // 确保进度条走完
        callback(1.0);

        Ok(())
    }
}
