//! SSH执行器模块
//! 提供SSH连接管理和命令执行能力
//!
//! 使用 russh 库实现真实的 SSH 连接和命令执行

use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use async_trait::async_trait;
use russh::client;
use russh::client::Config;
use russh::ChannelMsg;
use russh_keys::key::PublicKey;
use russh_keys::load_secret_key;

use crate::error::AppError;

/// SSH连接配置
#[derive(Debug, Clone)]
pub struct SSHConfig {
    /// 主机地址
    pub host: String,
    /// 端口
    pub port: u16,
    /// 用户名
    pub username: String,
    /// 认证方式
    pub auth: SSHAuth,
    /// 连接超时（秒）
    pub connect_timeout_secs: u64,
    /// 握手超时（秒）
    pub handshake_timeout_secs: u64,
    /// 命令执行超时（秒）
    pub command_timeout_secs: u64,
}

/// SSH认证方式
#[derive(Debug, Clone)]
pub enum SSHAuth {
    /// 密码认证
    Password(String),
    /// 私钥认证
    Key {
        /// 私钥内容（PEM格式）
        private_key: String,
        /// 密码（如果有）
        passphrase: Option<String>,
    },
}

/// SSH执行结果
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// 退出码
    pub exit_code: i32,
    /// 标准输出
    pub stdout: String,
    /// 标准错误
    pub stderr: String,
    /// 执行时长（秒）
    pub duration_secs: f64,
    /// 是否超时
    pub timed_out: bool,
}

/// SSH客户端
pub struct SSHClient {
    config: SSHConfig,
}

/// SSH 客户端会话处理器
struct SSHSession;

#[async_trait]
impl client::Handler for SSHSession {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        // 在生产环境中，应该验证服务器公钥
        // 这里暂时接受所有服务器密钥（仅用于开发）
        Ok(true)
    }
}

impl SSHClient {
    /// 创建新的SSH客户端
    pub fn new(config: SSHConfig) -> Self {
        Self { config }
    }

    /// 执行命令
    pub async fn execute(&self, command: &str) -> Result<ExecutionResult, AppError> {
        let start_time = std::time::Instant::now();

        debug!(
            host = %self.config.host,
            port = %self.config.port,
            user = %self.config.username,
            command = %command,
            "Executing SSH command"
        );

        // 创建 SSH 客户端配置
        let client_config = Arc::new(Config {
            preferred: russh::Preferred::default(),
            ..Default::default()
        });

        // 建立连接
        let mut handle = timeout(
            Duration::from_secs(self.config.handshake_timeout_secs),
            client::connect(
                client_config,
                (self.config.host.clone(), self.config.port),
                SSHSession,
            ),
        )
        .await
        .map_err(|_| {
            AppError::SshConnectionError(format!(
                "连接超时: {}@{}:{}",
                self.config.username, self.config.host, self.config.port
            ))
        })?
        .map_err(|e| {
            error!(error = %e, "SSH连接失败");
            AppError::SshConnectionError(format!("SSH连接失败: {}", e))
        })?;

        // 认证
        let auth_result = match &self.config.auth {
            SSHAuth::Password(password) => {
                handle
                    .authenticate_password(self.config.username.clone(), password)
                    .await
            }
            SSHAuth::Key {
                private_key,
                passphrase,
            } => {
                // 使用 load_secret_key 加载私钥
                let key = load_secret_key(private_key, passphrase.as_deref()).map_err(|e| {
                    error!(error = %e, "加载SSH私钥失败");
                    AppError::SshConnectionError(format!("加载私钥失败: {}", e))
                })?;

                handle
                    .authenticate_publickey(self.config.username.clone(), Arc::new(key))
                    .await
            }
        };

        if !auth_result.unwrap_or(false) {
            error!("SSH认证失败");
            return Err(AppError::SshAuthenticationError("SSH认证失败".to_string()));
        }

        info!("SSH认证成功，准备执行命令");

        // 执行命令
        let mut channel = handle.channel_open_session().await.map_err(|e| {
            error!(error = %e, "打开SSH通道失败");
            AppError::SshConnectionError(format!("打开SSH通道失败: {}", e))
        })?;

        channel.exec(true, command).await.map_err(|e| {
            error!(error = %e, "执行命令失败");
            AppError::SshExecutionError(format!("执行命令失败: {}", e))
        })?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = 0;

        // 读取输出
        let command_timeout = Duration::from_secs(self.config.command_timeout_secs);

        loop {
            let msg = timeout(command_timeout, channel.wait()).await;

            match msg {
                Ok(Some(ChannelMsg::Data { ref data })) => {
                    stdout.extend_from_slice(data);
                }
                Ok(Some(ChannelMsg::ExtendedData { ref data, ext })) => {
                    if ext == 1 {
                        // SSH_EXTENDED_DATA_STDERR
                        stderr.extend_from_slice(data);
                    }
                }
                Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                    exit_code = exit_status as i32;
                    break;
                }
                Ok(Some(ChannelMsg::Eof)) => {
                    break;
                }
                Ok(None) => {
                    break;
                }
                Err(_) => {
                    warn!("命令执行超时");
                    exit_code = 124; // timeout 退出码
                    break;
                }
                _ => {}
            }
        }

        // 关闭通道和连接
        let _ = channel.close().await;
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;

        let duration_secs = start_time.elapsed().as_secs_f64();
        let timed_out = exit_code == 124;

        info!(
            host = %self.config.host,
            exit_code = exit_code,
            duration_secs = duration_secs,
            stdout_len = stdout.len(),
            stderr_len = stderr.len(),
            "Command executed"
        );

        Ok(ExecutionResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
            duration_secs,
            timed_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_config_creation() {
        let config = SSHConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "test".to_string(),
            auth: SSHAuth::Password("password".to_string()),
            connect_timeout_secs: 10,
            handshake_timeout_secs: 10,
            command_timeout_secs: 30,
        };

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_ssh_auth_password() {
        let auth = SSHAuth::Password("test123".to_string());
        match auth {
            SSHAuth::Password(pwd) => assert_eq!(pwd, "test123"),
            _ => panic!("Expected Password auth"),
        }
    }

    #[test]
    fn test_ssh_auth_key() {
        let auth = SSHAuth::Key {
            private_key: "test-key".to_string(),
            passphrase: Some("pass".to_string()),
        };
        match auth {
            SSHAuth::Key {
                private_key,
                passphrase,
            } => {
                assert_eq!(private_key, "test-key");
                assert_eq!(passphrase, Some("pass".to_string()));
            }
            _ => panic!("Expected Key auth"),
        }
    }

    #[test]
    fn test_execution_result_creation() {
        let result = ExecutionResult {
            exit_code: 0,
            stdout: "test output".to_string(),
            stderr: String::new(),
            duration_secs: 1.5,
            timed_out: false,
        };

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "test output");
        assert!(!result.timed_out);
    }
}
