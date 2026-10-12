//! SSH执行器模块
//! 提供SSH连接管理和命令执行能力
//!
//! 使用 russh 库实现真实的 SSH 连接和命令执行

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

// 使用 base64 crate 进行脚本编码
use base64::{Engine as _, engine::general_purpose};

use russh::ChannelMsg;
use russh::client;
use russh::client::Config;
use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::PublicKeyBase64;
use russh::keys::PublicKeyOrCertificate;
use russh::keys::decode_secret_key;
use sha2::Digest;

use crate::error::AppError;

// 重新导出 common 的类型
pub use common::{execution::ExecutionResult, ssh::*};

/// 进度回调函数类型
/// 参数: (当前输出片段, 是否完成)
pub type ProgressCallback = Arc<dyn Fn(String, bool) + Send + Sync>;

/// SSH客户端
pub struct SSHClient {
    config: SshConfig,
    /// Accept 模式下首次接受的主机密钥（host:port, fingerprint），
    /// 由调用方负责持久化，使 TOFU 真正生效（此前接受后不落盘，
    /// 下次连接照样接受新密钥，中间人防护为零）
    accepted_key: Arc<Mutex<Option<(String, String)>>>,
}

impl SSHClient {
    /// 从 common 的 SshConfig 创建 SSH 客户端
    pub fn new(config: SshConfig) -> Self {
        Self {
            config,
            accepted_key: Arc::new(Mutex::new(None)),
        }
    }

    /// 从 host, username 和 password 创建客户端
    pub fn with_password(host: String, username: String, password: String) -> Self {
        Self::new(SshConfig::with_password(host, username, password))
    }

    /// 从 host, username 和 private key 创建客户端
    pub fn with_key(
        host: String,
        username: String,
        private_key: String,
        passphrase: Option<String>,
    ) -> Self {
        Self::new(SshConfig::with_key(host, username, private_key, passphrase))
    }

    /// 获取配置的引用
    pub fn config(&self) -> &SshConfig {
        &self.config
    }

    /// 取出本次连接中 Accept 模式首次接受的主机密钥（若有）。
    /// 返回 (host:port, sha256指纹hex)。仅在密钥为"首次接受的新密钥"时非空。
    pub fn take_accepted_host_key(&self) -> Option<(String, String)> {
        self.accepted_key.lock().unwrap().take()
    }

    /// 创建带验证策略的会话处理器
    fn create_session(&self) -> SSHSession {
        SSHSession {
            verification_mode: self.config.host_key_verification.clone(),
            known_hosts: self.config.known_hosts.clone(),
            host: self.config.host.clone(),
            port: self.config.port,
            accepted_key: self.accepted_key.clone(),
        }
    }

    /// 建立连接并完成认证（三个执行入口共用）
    async fn open_session(&self) -> Result<client::Handle<SSHSession>, AppError> {
        let client_config = Arc::new(Config {
            preferred: russh::Preferred::default(),
            ..Default::default()
        });

        let overall_timeout =
            std::cmp::min(self.config.connect_timeout_secs, self.config.handshake_timeout_secs);
        let session = self.create_session();

        let mut handle = timeout(
            Duration::from_secs(overall_timeout),
            client::connect(client_config, (self.config.host.clone(), self.config.port), session),
        )
        .await
        .map_err(|_| {
            if self.config.connect_timeout_secs <= self.config.handshake_timeout_secs {
                AppError::SshConnectionError(format!(
                    "TCP连接超时: {}@{}",
                    self.config.host, self.config.port
                ))
            } else {
                AppError::SshConnectionError(format!(
                    "SSH握手超时: {}@{}:{}",
                    self.config.username, self.config.host, self.config.port
                ))
            }
        })?
        .map_err(|e| {
            error!(error = %e, "SSH连接失败");
            if e.to_string().contains("Host key") || e.to_string().contains("fingerprint") {
                AppError::SshConnectionError(format!("主机密钥验证失败: {}", e))
            } else {
                AppError::SshConnectionError(format!("SSH连接失败: {}", e))
            }
        })?;

        // 认证
        let auth = convert_auth(&self.config.auth);
        let auth_result = match auth {
            InternalSshAuth::Password(password) => {
                handle
                    .authenticate_password(self.config.username.clone(), &password)
                    .await
            }
            InternalSshAuth::Key {
                private_key,
                passphrase,
            } => {
                let key =
                    decode_secret_key(&private_key, passphrase.as_deref()).map_err(|e| {
                        error!(error = %e, "加载SSH私钥失败");
                        AppError::SshConnectionError(format!("加载私钥失败: {}", e))
                    })?;

                handle
                    .authenticate_publickey(
                        self.config.username.clone(),
                        PrivateKeyWithHashAlg::new(Arc::new(key), None),
                    )
                    .await
            }
        };

        if !auth_result.map(|result| result.success()).unwrap_or(false) {
            error!("SSH认证失败");
            return Err(AppError::SshAuthenticationError("SSH认证失败".to_string()));
        }

        Ok(handle)
    }

    /// 执行命令
    pub async fn execute(&self, command: &str) -> Result<ExecutionResult, AppError> {
        let start_time = Instant::now();

        debug!(
            host = %self.config.host,
            port = %self.config.port,
            user = %self.config.username,
            command = %command,
            "Executing SSH command"
        );

        let mut handle = self.open_session().await?;
        let result = run_command_on_channel(&mut handle, command, self.command_deadline(), None)
            .await?;

        self.finish_session(&mut handle).await;
        self.log_result("Command executed", &result, start_time.elapsed().as_secs_f64());
        Ok(result)
    }

    /// 执行命令并支持增量输出推送
    pub async fn execute_with_progress(
        &self,
        command: &str,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<ExecutionResult, AppError> {
        let start_time = Instant::now();

        debug!(
            host = %self.config.host,
            port = %self.config.port,
            user = %self.config.username,
            command = %command,
            "Executing SSH command with progress"
        );

        let mut handle = self.open_session().await?;
        let result =
            run_command_on_channel(&mut handle, command, self.command_deadline(), progress_callback)
                .await?;

        self.finish_session(&mut handle).await;
        self.log_result(
            "Command executed with progress",
            &result,
            start_time.elapsed().as_secs_f64(),
        );
        Ok(result)
    }

    /// 执行脚本（通过上传临时脚本文件）
    pub async fn execute_script(
        &self,
        script_content: &str,
        script_path: Option<&str>,
    ) -> Result<ExecutionResult, AppError> {
        let start_time = Instant::now();

        debug!(
            host = %self.config.host,
            port = %self.config.port,
            user = %self.config.username,
            script_len = script_content.len(),
            "Executing SSH script"
        );

        // 生成临时脚本文件路径
        let temp_script_path = if let Some(path) = script_path {
            path.to_string()
        } else {
            // 使用 /tmp 目录和随机名称
            format!("/tmp/ops_script_{}.sh", uuid::Uuid::new_v4().to_string().replace("-", ""))
        };

        // 创建脚本执行命令（包含上传和执行）
        // 使用 base64 编码脚本内容以避免转义问题
        let encoded_script = general_purpose::STANDARD.encode(script_content);
        let command = format!(
            "echo '{}' | base64 -d > '{}' && chmod +x '{}' && sh '{}'; rm -f '{}'",
            encoded_script, temp_script_path, temp_script_path, temp_script_path, temp_script_path
        );

        let mut handle = self.open_session().await?;
        let result = run_command_on_channel(&mut handle, &command, self.command_deadline(), None)
            .await?;

        self.finish_session(&mut handle).await;
        self.log_result("Script executed", &result, start_time.elapsed().as_secs_f64());
        Ok(result)
    }

    fn command_deadline(&self) -> Instant {
        Instant::now() + Duration::from_secs(self.config.command_timeout_secs)
    }

    async fn finish_session(&self, handle: &mut client::Handle<SSHSession>) {
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "").await;
    }

    fn log_result(&self, label: &str, result: &ExecutionResult, duration_secs: f64) {
        info!(
            host = %self.config.host,
            exit_code = result.exit_code,
            duration_secs = duration_secs,
            stdout_len = result.stdout.len(),
            stderr_len = result.stderr.len(),
            "{}",
            label
        );
    }
}

/// 在已打开的通道上执行命令并收集输出。
///
/// - `deadline` 是整个命令的墙钟超时（此前对每次 `channel.wait()` 单独
///   计时，等价于"不活动超时"：每 5 分钟吐一行输出的命令永不超时）；
/// - 收到 EOF 后继续等待退出状态（部分服务器先 EOF 后 ExitStatus，
///   旧行为在 EOF 直接 break 会把失败命令记成 exit 0）；
/// - 超时把退出码置为 124 并主动关闭通道（远端进程通常会随通道关闭终止）；
/// - 累计输出超过 10 MiB 后丢弃后续内容（防止大输出撑爆内存）。
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;
const EOF_EXIT_WAIT: Duration = Duration::from_secs(5);

async fn run_command_on_channel(
    handle: &mut client::Handle<SSHSession>,
    command: &str,
    deadline: Instant,
    progress_callback: Option<ProgressCallback>,
) -> Result<ExecutionResult, AppError> {
    let start_time = Instant::now();

    let mut channel = handle.channel_open_session().await.map_err(|e| {
        error!(error = %e, "打开SSH通道失败");
        AppError::SshConnectionError(format!("打开SSH通道失败: {}", e))
    })?;

    channel.exec(true, command).await.map_err(|e| {
        error!(error = %e, "执行命令失败");
        AppError::SshExecutionError(format!("执行命令失败: {}", e))
    })?;

    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut exit_code = 0;
    let mut timed_out = false;
    let mut output_truncated = false;
    let mut last_callback_time = Instant::now();
    let callback_interval = Duration::from_millis(500);

    // 最大保留 500ms 的余量给清理逻辑
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            warn!("命令执行超时（整体墙钟超时）");
            exit_code = 124;
            timed_out = true;
            break;
        }

        let msg = timeout(remaining, channel.wait()).await;
        match msg {
            Ok(Some(ChannelMsg::Data { ref data })) => {
                if stdout.len() + stderr.len() < MAX_OUTPUT_BYTES {
                    stdout.extend_from_slice(data);
                } else if !output_truncated {
                    output_truncated = true;
                    warn!("命令输出超过 10MiB，丢弃后续输出");
                }

                // 增量推送输出
                if let Some(ref callback) = progress_callback {
                    let now = Instant::now();
                    if now.duration_since(last_callback_time) >= callback_interval {
                        let output = String::from_utf8_lossy(&stdout).to_string();
                        callback(output, false);
                        last_callback_time = now;
                    }
                }
            }
            Ok(Some(ChannelMsg::ExtendedData { ref data, ext })) => {
                if ext == 1 && stdout.len() + stderr.len() < MAX_OUTPUT_BYTES {
                    // SSH_EXTENDED_DATA_STDERR
                    stderr.extend_from_slice(data);
                }
            }
            Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                exit_code = exit_status as i32;
                break;
            }
            Ok(Some(ChannelMsg::Eof)) => {
                // 等待退出状态：旧实现在 EOF 直接 break，会把"先 EOF 后
                // ExitStatus"的失败命令记成 exit 0
                let mut got_exit = false;
                let eof_deadline = Instant::now() + EOF_EXIT_WAIT;
                loop {
                    let remaining = eof_deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match timeout(remaining, channel.wait()).await {
                        Ok(Some(ChannelMsg::ExitStatus { exit_status })) => {
                            exit_code = exit_status as i32;
                            got_exit = true;
                            break;
                        }
                        Ok(Some(ChannelMsg::Data { ref data })) => {
                            if stdout.len() + stderr.len() < MAX_OUTPUT_BYTES {
                                stdout.extend_from_slice(data);
                            }
                        }
                        Ok(Some(ChannelMsg::ExtendedData { ref data, ext })) => {
                            if ext == 1 && stdout.len() + stderr.len() < MAX_OUTPUT_BYTES {
                                stderr.extend_from_slice(data);
                            }
                        }
                        Ok(None) => break,
                        Ok(Some(_)) => continue,
                        Err(_) => break,
                    }
                }
                if !got_exit {
                    debug!("EOF 后未收到退出状态，按通道关闭处理");
                }
                break;
            }
            Ok(None) => {
                break;
            }
            Err(_) => {
                warn!("命令执行超时（整体墙钟超时）");
                exit_code = 124;
                timed_out = true;
                break;
            }
            _ => {}
        }
    }

    // 关闭通道与连接
    let _ = channel.close().await;

    let duration_secs = start_time.elapsed().as_secs_f64();

    if let Some(ref callback) = progress_callback {
        let final_output = if stderr.is_empty() {
            String::from_utf8_lossy(&stdout).to_string()
        } else if stdout.is_empty() {
            String::from_utf8_lossy(&stderr).to_string()
        } else {
            format!(
                "{}\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            )
        };
        callback(final_output, true);
    }

    Ok(ExecutionResult {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
        duration_secs,
        timed_out,
    })
}

/// SSH 客户端会话处理器
struct SSHSession {
    verification_mode: HostKeyVerification,
    known_hosts: Option<std::collections::HashMap<String, String>>,
    host: String,
    port: u16,
    accepted_key: Arc<Mutex<Option<(String, String)>>>,
}

impl client::Handler for SSHSession {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let verification_mode = self.verification_mode.clone();
        let known_hosts = self.known_hosts.clone();
        let host = self.host.clone();
        let port = self.port;
        let accepted_key = self.accepted_key.clone();
        let key_data = server_public_key.public_key().public_key_base64();

        async move {
            match verification_mode {
                HostKeyVerification::Disabled => {
                    warn!(
                        host = %host,
                        port = port,
                        "Host key verification DISABLED - accepting all keys"
                    );
                    Ok(true)
                }
                HostKeyVerification::Accept => {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(key_data.as_bytes());
                    let fingerprint = hex::encode(hasher.finalize());
                    let host_key = format!("{}:{}", host, port);

                    if let Some(known_hosts) = &known_hosts
                        && let Some(stored_fingerprint) = known_hosts.get(&host_key)
                    {
                        if stored_fingerprint == &fingerprint {
                            debug!(host = %host_key, "Host key verified");
                            return Ok(true);
                        }

                        error!(
                            host = %host_key,
                            expected = %stored_fingerprint,
                            actual = %fingerprint,
                            "Host key mismatch - POSSIBLE SECURITY BREACH"
                        );
                        return Ok(false);
                    }

                    info!(
                        host = %host_key,
                        fingerprint = %fingerprint,
                        "First time connecting - accepting host key (record for persistence)"
                    );
                    // 记录首次接受的密钥，由调用方持久化到主机记录，
                    // 否则 TOFU 不产生任何约束力
                    *accepted_key.lock().unwrap() = Some((host_key, fingerprint));
                    Ok(true)
                }
                HostKeyVerification::Strict => {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(key_data.as_bytes());
                    let fingerprint = hex::encode(hasher.finalize());
                    let host_key = format!("{}:{}", host, port);

                    if let Some(known_hosts) = &known_hosts
                        && let Some(stored_fingerprint) = known_hosts.get(&host_key)
                    {
                        if stored_fingerprint == &fingerprint {
                            debug!(host = %host_key, "Host key verified (strict mode)");
                            return Ok(true);
                        }

                        error!(
                            host = %host_key,
                            expected = %stored_fingerprint,
                            actual = %fingerprint,
                            "Host key mismatch - REJECTING CONNECTION"
                        );
                        return Ok(false);
                    }

                    error!(
                        host = %host_key,
                        "Unknown host in strict mode - rejecting connection"
                    );
                    Ok(false)
                }
            }
        }
    }
}

/// 内部使用的认证方式（与 common 的 SshAuth 相同结构，但使用引用）
enum InternalSshAuth {
    Password(String),
    Key {
        private_key: String,
        passphrase: Option<String>,
    },
}

fn convert_auth(auth: &SshAuth) -> InternalSshAuth {
    match auth {
        SshAuth::Password { password } => InternalSshAuth::Password(password.clone()),
        SshAuth::Key {
            private_key,
            passphrase,
        } => InternalSshAuth::Key {
            private_key: private_key.clone(),
            passphrase: passphrase.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_client_with_password() {
        let client = SSHClient::with_password(
            "localhost".to_string(),
            "test".to_string(),
            "password".to_string(),
        );

        assert_eq!(client.config().host, "localhost");
        assert_eq!(client.config().username, "test");
    }

    #[test]
    fn test_ssh_client_with_key() {
        let client = SSHClient::with_key(
            "example.com".to_string(),
            "user".to_string(),
            "private-key".to_string(),
            Some("passphrase".to_string()),
        );

        assert_eq!(client.config().host, "example.com");
        assert_eq!(client.config().username, "user");
    }

    #[test]
    fn test_ssh_config_from_common() {
        let config = SshConfig::with_password(
            "test.com".to_string(),
            "admin".to_string(),
            "secret".to_string(),
        )
        .with_port(2222);

        assert_eq!(config.host, "test.com");
        assert_eq!(config.port, 2222);
    }

    #[test]
    fn test_execution_result_from_common() {
        let result = ExecutionResult::success("output".to_string(), 1.5);
        assert!(result.is_success());

        let failed = ExecutionResult::failure(1, "err".to_string(), "error".to_string(), 2.0);
        assert!(failed.is_failure());

        let timed_out = ExecutionResult::timeout(30.0);
        assert!(timed_out.timed_out);
    }
}
