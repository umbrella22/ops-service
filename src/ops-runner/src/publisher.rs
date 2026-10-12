//! RabbitMQ 消息发布器 - 向控制面发送状态和日志

use anyhow::{Context, Result};
use lapin::types::FieldTable;
use lapin::types::ShortString;
use lapin::{BasicProperties, Channel, ExchangeKind, options::*};
use tracing::{debug, error, info, warn};

use crate::config::{ArtifactStorageConfig, RunnerConfig};
use crate::messages::*;

fn short_string(value: impl Into<String>) -> ShortString {
    value.into().into()
}

/// 消息发布器
pub struct MessagePublisher {
    channel: Channel,
    runner_name: String,
    exchange: String,
}

impl MessagePublisher {
    /// 创建新的发布器
    pub async fn new(config: &RunnerConfig, channel: Channel) -> Result<Self> {
        let exchange = config.message_queue.exchange.clone();
        let runner_name = config.runner.name.clone();

        // 启用 publisher confirm：publish_with_confirm 依赖 broker 的确认回执。
        // 未启用时 lapin 立即返回 Confirmation::NotRequested，is_ack() 为 false，
        // 所有发布都会被误判为 "Broker nacked"（消息实际以 fire-and-forget 发出）。
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .context("Failed to enable publisher confirms")?;

        // 声明统一的交换机（Topic 类型）
        // 与控制面保持一致：使用同一个 exchange，通过 routing key 区分消息类型
        channel
            .exchange_declare(
                short_string(exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("Failed to declare build exchange")?;

        info!("Message publisher created for runner: {}, exchange: {}", runner_name, exchange);

        Ok(Self {
            channel,
            runner_name,
            exchange,
        })
    }

    /// 发布构建状态
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_build_status(
        &self,
        task: &BuildTaskMessage,
        status: BuildStatus,
        step_status: Option<StepStatusUpdate>,
        error: Option<String>,
        error_category: Option<ErrorCategory>,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        let status_str = format!("{:?}", status);

        let message = BuildStatusMessage {
            task_id: task.task_id,
            job_id: task.job_id,
            runner_name: self.runner_name.clone(),
            attempt_id: Some(attempt_id),
            status: status.clone(),
            step_status,
            error,
            error_category,
            timestamp: chrono::Utc::now(),
        };

        // 使用与控制面消费者一致的 routing key 格式: build.status.{job_id}.{task_id}
        // 控制面消费者绑定到 "build.status.#"
        let routing_key = format!("build.status.{}.{}", task.job_id, task.task_id);
        let payload = serde_json::to_vec(&message).context("Failed to serialize status message")?;

        publish_with_confirm(
            &self.channel,
            &self.exchange,
            &routing_key,
            &payload,
            "status message",
        )
        .await?;

        debug!(
            "Published status: job={}, task={}, status={}, routing_key={}",
            task.job_id, task.task_id, status_str, routing_key
        );

        Ok(())
    }

    /// 发布步骤状态
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_step_status(
        &self,
        task: &BuildTaskMessage,
        step: &BuildStep,
        step_status: StepStatus,
        started_at: chrono::DateTime<chrono::Utc>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        exit_code: Option<i32>,
        artifact: Option<BuildArtifact>,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        let step_status_str = format!("{:?}", step_status);

        let step_update = StepStatusUpdate {
            step_id: step.id.clone(),
            status: step_status,
            started_at,
            completed_at,
            exit_code,
            artifact,
        };

        self.publish_build_status(
            task,
            BuildStatus::Running,
            Some(step_update),
            None,
            None,
            attempt_id,
        )
        .await?;

        debug!(
            "Published step status: job={}, task={}, step={}, status={}",
            task.job_id, task.task_id, step.name, step_status_str
        );

        Ok(())
    }

    /// 发布日志
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_log(
        &self,
        task: &BuildTaskMessage,
        step: &BuildStep,
        content: &str,
        level: LogLevel,
        offset: u64,
        is_final: bool,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        // 分割大日志以避免超过 RabbitMQ 消息大小限制
        const MAX_CHUNK_SIZE: usize = 256 * 1024; // 256KB

        let content_bytes = content.as_bytes();

        if content_bytes.len() <= MAX_CHUNK_SIZE {
            self.publish_log_chunk(task, step, content, level.clone(), offset, 0, is_final, attempt_id)
                .await?;
        } else {
            // 按字符边界分块发送，避免把多字节字符（如中文）截断成无效字节
            let chunks = split_log_chunks(content, MAX_CHUNK_SIZE);
            let total_chunks = chunks.len();
            for (i, (chunk_offset, chunk)) in chunks.iter().enumerate() {
                let is_last_chunk = i == total_chunks - 1;
                self.publish_log_chunk(
                    task,
                    step,
                    chunk,
                    level.clone(),
                    offset + *chunk_offset as u64,
                    i as u64,
                    is_final && is_last_chunk,
                    attempt_id,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// 发布单个日志块
    #[allow(clippy::too_many_arguments)]
    async fn publish_log_chunk(
        &self,
        task: &BuildTaskMessage,
        step: &BuildStep,
        content: &str,
        level: LogLevel,
        offset: u64,
        chunk_index: u64,
        is_final: bool,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        let message = BuildLogMessage {
            task_id: task.task_id,
            job_id: task.job_id,
            step_id: step.id.clone(),
            runner_name: self.runner_name.clone(),
            attempt_id: Some(attempt_id),
            level,
            content: content.to_string(),
            offset,
            chunk_index,
            is_final,
            timestamp: chrono::Utc::now(),
        };

        // 使用与控制面消费者一致的 routing key 格式: build.log.{job_id}.{task_id}.{step_id}
        // 控制面消费者绑定到 "build.log.#"
        let routing_key = format!("build.log.{}.{}.{}", task.job_id, task.task_id, step.id);

        let payload = serde_json::to_vec(&message).context("Failed to serialize log message")?;

        publish_with_confirm(
            &self.channel,
            &self.exchange,
            &routing_key,
            &payload,
            "log message",
        )
        .await?;

        debug!(
            "Published log: job={}, task={}, step={}, bytes={}, offset={}, chunk_index={}, routing_key={}",
            task.job_id,
            task.task_id,
            step.name,
            content.len(),
            offset,
            chunk_index,
            routing_key
        );

        Ok(())
    }

    /// 发布错误信息
    pub async fn publish_error(
        &self,
        task: &BuildTaskMessage,
        error: &str,
        category: ErrorCategory,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        let category_str = format!("{:?}", category);

        self.publish_build_status(
            task,
            BuildStatus::Failed,
            None,
            Some(error.to_string()),
            Some(category.clone()),
            attempt_id,
        )
        .await?;

        error!(
            "Published error: job={}, task={}, error={}, category={}",
            task.job_id, task.task_id, error, category_str
        );

        Ok(())
    }

    /// 发布产物信息
    pub async fn publish_artifact(
        &self,
        task: &BuildTaskMessage,
        step: &BuildStep,
        artifact: &BuildArtifact,
        attempt_id: uuid::Uuid,
    ) -> Result<()> {
        // 产物信息作为状态消息的一部分发布
        let step_update = StepStatusUpdate {
            step_id: step.id.clone(),
            status: StepStatus::Succeeded,
            started_at: chrono::Utc::now(),
            completed_at: Some(chrono::Utc::now()),
            exit_code: Some(0),
            artifact: Some(artifact.clone()),
        };

        let message = BuildStatusMessage {
            task_id: task.task_id,
            job_id: task.job_id,
            runner_name: self.runner_name.clone(),
            attempt_id: Some(attempt_id),
            status: BuildStatus::Running,
            step_status: Some(step_update),
            error: None,
            error_category: None,
            timestamp: chrono::Utc::now(),
        };

        // 使用状态消息的 routing key 格式
        let routing_key = format!("build.status.{}.{}", task.job_id, task.task_id);

        let payload =
            serde_json::to_vec(&message).context("Failed to serialize artifact message")?;

        publish_with_confirm(
            &self.channel,
            &self.exchange,
            &routing_key,
            &payload,
            "artifact message",
        )
        .await?;

        info!(
            "Published artifact: job={}, task={}, artifact={}, size={}, routing_key={}",
            task.job_id, task.task_id, artifact.name, artifact.size, routing_key
        );

        Ok(())
    }
}

/// 发布并等待 broker 确认（publisher confirm），确保发布失败能被感知
async fn publish_with_confirm(
    channel: &Channel,
    exchange: &str,
    routing_key: &str,
    payload: &[u8],
    description: &str,
) -> Result<()> {
    let confirm = channel
        .basic_publish(
            short_string(exchange.to_string()),
            short_string(routing_key.to_string()),
            BasicPublishOptions::default(),
            payload,
            BasicProperties::default()
                .with_delivery_mode(2) // 持久化
                .with_content_type("application/json".into()),
        )
        .await
        .with_context(|| format!("Failed to publish {}", description))?
        .await
        .with_context(|| format!("Broker did not confirm the {}", description))?;

    if !confirm.is_ack() {
        anyhow::bail!("Broker nacked the {} (routing key: {})", description, routing_key);
    }

    Ok(())
}

/// 按字符边界把内容切分为不超过 max_size 字节的块，返回 (块起始字节偏移, 块内容)。
///
/// 直接按字节切分会把多字节字符（如中文）截断，导致 `from_utf8` 失败、整块日志丢失；
/// 这里沿 `char_indices` 累积，保证每个块都是合法 UTF-8。
fn split_log_chunks(content: &str, max_size: usize) -> Vec<(usize, &str)> {
    debug_assert!(max_size >= 4, "max_size must hold at least one char");

    let mut chunks = Vec::new();
    let mut chunk_start = 0usize;

    for (idx, ch) in content.char_indices() {
        if idx + ch.len_utf8() - chunk_start > max_size {
            chunks.push((chunk_start, &content[chunk_start..idx]));
            chunk_start = idx;
        }
    }

    chunks.push((chunk_start, &content[chunk_start..]));
    chunks
}

/// 校验远端存储路径，防止路径穿越。
///
/// 拒绝以 '/' 开头、包含 ".."、空组件或反斜杠的路径。
fn validate_remote_path(remote_path: &str) -> Result<()> {
    if remote_path.is_empty()
        || remote_path.starts_with('/')
        || remote_path.starts_with('\\')
        || remote_path.contains('\\')
        || remote_path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        anyhow::bail!("Invalid remote path: {:?}", remote_path);
    }
    Ok(())
}

/// 产物存储客户端
pub struct ArtifactStorage {
    /// 存储类型 (s3, minio, http, local)
    pub storage_type: String,
    /// 存储端点
    endpoint: Option<String>,
    /// 存储桶/路径前缀
    bucket: Option<String>,
    /// 访问密钥
    access_key: Option<String>,
    /// 密钥
    secret_key: Option<String>,
    /// S3 区域
    region: String,
    /// HTTP 客户端（用于上传）
    client: reqwest::Client,
}

impl ArtifactStorage {
    /// 创建新的存储客户端
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage_type: String,
        endpoint: Option<String>,
        bucket: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
        region: String,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap();

        Self {
            storage_type,
            endpoint,
            bucket,
            access_key,
            secret_key,
            region,
            client,
        }
    }

    /// 从配置创建
    pub fn from_config(config: &ArtifactStorageConfig) -> Self {
        Self::new(
            config.storage_type.clone(),
            config.endpoint.clone(),
            config.bucket.clone(),
            config.access_key.clone(),
            config.secret_key.clone(),
            config.region.clone(),
        )
    }

    /// 上传产物
    pub async fn upload(
        &self,
        artifact_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<ArtifactUploadResult> {
        // 所有存储后端共用远端路径，先做防御性校验，防止路径穿越
        validate_remote_path(remote_path)?;

        match self.storage_type.as_str() {
            "s3" | "minio" => self.upload_s3(artifact_path, remote_path).await,
            "http" => self.upload_http(artifact_path, remote_path).await,
            "local" => self.upload_local(artifact_path, remote_path).await,
            _ => {
                warn!("Unknown storage type: {}, using local", self.storage_type);
                self.upload_local(artifact_path, remote_path).await
            }
        }
    }

    /// 上传到 S3 兼容存储
    ///
    /// 使用 AWS Signature V4 对请求签名（此前误用 HTTP Basic Auth，真实 S3 会拒绝）。
    async fn upload_s3(
        &self,
        artifact_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<ArtifactUploadResult> {
        let key_id = self
            .access_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("S3 access key not configured"))?;
        let secret = self
            .secret_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("S3 secret key not configured"))?;

        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("S3 endpoint not configured"))?;

        let bucket = self.bucket.as_deref().unwrap_or("artifacts");

        // 构造 S3 URL
        let url = format!("{}/{}/{}", endpoint.trim_end_matches('/'), bucket, remote_path);

        // 流式计算请求体（文件内容）的 SHA256，作为 SigV4 payload hash
        let payload_hash = crate::executor::sha256_file_hex(artifact_path).await?;

        let extra_headers =
            vec![("Content-Type".to_string(), "application/octet-stream".to_string())];
        let signed = common::sigv4::signed_headers(
            "PUT",
            &url,
            &self.region,
            "s3",
            key_id,
            secret,
            &payload_hash,
            &extra_headers,
        )
        .map_err(|e| anyhow::anyhow!("Failed to sign S3 request: {}", e))?;

        // 单次 PUT 请求体需要整体缓冲（S3 单次 PUT 上限 5 GiB；分片上传不在本期范围）
        let content = tokio::fs::read(artifact_path)
            .await
            .context("Failed to read artifact file")?;

        let mut request = self.client.put(&url);
        for (name, value) in signed {
            request = request.header(name, value);
        }
        let response = request
            .body(content)
            .send()
            .await
            .context("Failed to upload to S3")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("S3 upload failed with status {}: {}", status, body);
        }

        Ok(ArtifactUploadResult {
            url,
            size: artifact_path.metadata()?.len(),
            sha256: payload_hash,
        })
    }

    /// 上传到 HTTP 端点
    async fn upload_http(
        &self,
        artifact_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<ArtifactUploadResult> {
        let content = tokio::fs::read(artifact_path)
            .await
            .context("Failed to read artifact file")?;

        let sha256 = {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(&content);
            hex::encode(hash)
        };

        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("HTTP endpoint not configured"))?;

        let url = format!("{}/{}", endpoint.trim_end_matches('/'), remote_path);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .header("X-Artifact-SHA256", &sha256)
            .body(content)
            .send()
            .await
            .context("Failed to upload to HTTP endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("HTTP upload failed with status {}: {}", status, body);
        }

        // 假设响应包含下载 URL
        let download_url = response
            .headers()
            .get("X-Download-URL")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&url)
            .to_string();

        Ok(ArtifactUploadResult {
            url: download_url,
            size: artifact_path.metadata()?.len(),
            sha256,
        })
    }

    /// 复制到本地存储
    async fn upload_local(
        &self,
        artifact_path: &std::path::Path,
        remote_path: &str,
    ) -> Result<ArtifactUploadResult> {
        let sha256 = crate::executor::sha256_file_hex(artifact_path).await?;

        let storage_dir = self.endpoint.as_deref().unwrap_or("/tmp/artifacts");

        // 创建目标目录（remote_path 已在 upload() 入口做过穿越校验）
        let target_path = std::path::PathBuf::from(storage_dir).join(remote_path);

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create artifact directory")?;
        }

        // 复制文件
        tokio::fs::copy(artifact_path, &target_path)
            .await
            .context("Failed to copy artifact")?;

        info!("Artifact saved locally: {:?}", target_path);

        Ok(ArtifactUploadResult {
            url: target_path.to_string_lossy().to_string(),
            size: artifact_path.metadata()?.len(),
            sha256,
        })
    }
}

/// 产物上传结果
#[derive(Debug, Clone)]
pub struct ArtifactUploadResult {
    /// 下载 URL
    pub url: String,
    /// 文件大小
    pub size: u64,
    /// SHA256 哈希
    #[allow(dead_code)]
    pub sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_storage_creation() {
        let storage = ArtifactStorage::new(
            "local".to_string(),
            Some("/tmp/artifacts".to_string()),
            None,
            None,
            None,
            "us-east-1".to_string(),
        );

        assert_eq!(storage.storage_type, "local");
        assert_eq!(storage.endpoint, Some("/tmp/artifacts".to_string()));
        assert_eq!(storage.region, "us-east-1");
    }

    #[test]
    fn test_upload_result_creation() {
        let result = ArtifactUploadResult {
            url: "https://storage.example.com/artifact.bin".to_string(),
            size: 1024,
            sha256: "abc123".to_string(),
        };

        assert_eq!(result.size, 1024);
        assert_eq!(result.url, "https://storage.example.com/artifact.bin");
    }

    #[test]
    fn test_split_log_chunks_ascii() {
        let content = "a".repeat(600 * 1024);
        let chunks = split_log_chunks(&content, 256 * 1024);
        assert_eq!(chunks.len(), 3);
        // 块偏移应该是连续的
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks[1].0, 256 * 1024);
        assert_eq!(chunks[2].0, 512 * 1024);
        // 拼接后应与原内容一致
        let joined: String = chunks.iter().map(|(_, c)| *c).collect();
        assert_eq!(joined, content);
        for (_, chunk) in &chunks {
            assert!(chunk.len() <= 256 * 1024);
        }
    }

    #[test]
    fn test_split_log_chunks_multibyte() {
        // 中文字符每个 3 字节，按字节切分会截断字符
        let content = "你好世界".repeat(100 * 1024); // 1.2 MB
        let chunks = split_log_chunks(&content, 256 * 1024);
        assert!(chunks.len() > 1);
        for (_, chunk) in &chunks {
            // 每个块必须是合法 UTF-8 且不超限
            assert!(std::str::from_utf8(chunk.as_bytes()).is_ok());
            assert!(chunk.len() <= 256 * 1024);
            // 不应存在残缺字符
            assert!(chunk.chars().all(|c| c != '\u{FFFD}'));
        }
        // 拼接后应与原内容一致（无字符丢失）
        let joined: String = chunks.iter().map(|(_, c)| *c).collect();
        assert_eq!(joined, content);
    }

    #[test]
    fn test_split_log_chunks_small() {
        let content = "hello".to_string();
        let chunks = split_log_chunks(&content, 256 * 1024);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (0, "hello"));
    }

    #[test]
    fn test_validate_remote_path() {
        assert!(validate_remote_path("proj/build/v1/file.jar").is_ok());
        assert!(validate_remote_path("a/b").is_ok()); // 多级合法路径
        assert!(validate_remote_path("a//b").is_err()); // 空组件
        assert!(validate_remote_path("/abs/path").is_err()); // 绝对路径
        assert!(validate_remote_path("a/../b").is_err()); // 父目录穿越
        assert!(validate_remote_path("..").is_err());
        assert!(validate_remote_path("").is_err());
        assert!(validate_remote_path("a\\b").is_err()); // 反斜杠
    }
}
