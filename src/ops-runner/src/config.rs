//! Runner 配置管理

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Runner 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    /// Runner 基本信息
    pub runner: RunnerInfo,

    /// 控制面配置
    pub control_plane: ControlPlaneConfig,

    /// RabbitMQ 配置
    pub message_queue: MessageQueueConfig,

    /// 执行配置
    pub execution: ExecutionConfig,

    /// 产物存储配置
    #[serde(default)]
    pub artifact_storage: ArtifactStorageConfig,
}

/// Runner 基本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerInfo {
    /// Runner 名称（唯一标识）
    pub name: String,

    /// Runner 标签/能力
    #[serde(default)]
    pub capabilities: Vec<String>,

    /// 是否支持 Docker 执行
    #[serde(default = "default_true")]
    pub docker_supported: bool,

    /// 最大并发任务数
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_jobs: usize,

    /// 出站白名单（域名）
    #[serde(default)]
    pub outbound_allowlist: Vec<String>,

    /// 环境类型（dev/staging/prod）
    pub environment: String,
}

fn default_true() -> bool {
    true
}

fn default_max_concurrent() -> usize {
    2
}

/// 控制面配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPlaneConfig {
    /// 控制 API 地址
    pub api_url: String,

    /// Runner API Key（用于注册和状态更新）
    pub api_key: String,

    /// 心跳间隔（秒）
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

fn default_heartbeat_interval() -> u64 {
    30
}

/// RabbitMQ 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageQueueConfig {
    /// AMQP 地址
    pub amqp_url: String,

    /// 虚拟主机
    #[serde(default = "default_vhost")]
    pub vhost: String,

    /// 交换机名称
    #[serde(default = "default_exchange")]
    pub exchange: String,

    /// 队列命名前缀
    #[serde(default = "default_queue_prefix")]
    pub queue_prefix: String,

    /// 预取消息数量
    #[serde(default = "default_prefetch")]
    pub prefetch: u16,

    /// 控制面下发的完整队列名（如 runner.<name>.queue），提供时优先使用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_name_override: Option<String>,

    /// 控制面下发的 routing key 模式（如 build.{name}，{..} 为占位符），提供时优先使用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_key_pattern: Option<String>,
}

fn default_vhost() -> String {
    "/".to_string()
}

fn default_exchange() -> String {
    "ops.build".to_string()
}

fn default_queue_prefix() -> String {
    "ops-runner".to_string()
}

fn default_prefetch() -> u16 {
    1
}

/// 执行配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// workspace 基础目录
    #[serde(default = "default_workspace_dir")]
    pub workspace_base_dir: String,

    /// 任务执行超时（秒）
    #[serde(default = "default_task_timeout")]
    pub task_timeout_secs: u64,

    /// 步骤执行超时（秒）
    #[serde(default = "default_step_timeout")]
    pub step_timeout_secs: u64,

    /// 是否清理 workspace
    #[serde(default = "default_true")]
    pub cleanup_workspace: bool,

    /// 构建缓存目录
    #[serde(default)]
    pub cache_dir: Option<String>,

    /// Docker 配置
    #[serde(default)]
    pub docker: Option<DockerConfig>,
}

/// 产物存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactStorageConfig {
    /// 存储类型 (s3, minio, http, local)
    #[serde(default = "default_storage_type")]
    pub storage_type: String,

    /// 存储端点（S3 兼容服务地址或本地存储目录）
    #[serde(default)]
    pub endpoint: Option<String>,

    /// 存储桶
    #[serde(default)]
    pub bucket: Option<String>,

    /// 访问密钥
    #[serde(default)]
    pub access_key: Option<String>,

    /// 私有密钥
    #[serde(default)]
    pub secret_key: Option<String>,

    /// S3 区域
    #[serde(default = "default_storage_region")]
    pub region: String,
}

impl Default for ArtifactStorageConfig {
    fn default() -> Self {
        Self {
            storage_type: default_storage_type(),
            endpoint: None,
            bucket: None,
            access_key: None,
            secret_key: None,
            region: default_storage_region(),
        }
    }
}

impl ArtifactStorageConfig {
    /// 从环境变量加载（与历史行为保持一致）
    pub fn from_env() -> Self {
        Self {
            storage_type: std::env::var("ARTIFACT_STORAGE_TYPE")
                .unwrap_or_else(|_| default_storage_type()),
            endpoint: std::env::var("ARTIFACT_STORAGE_ENDPOINT").ok(),
            bucket: std::env::var("ARTIFACT_STORAGE_BUCKET").ok(),
            access_key: std::env::var("ARTIFACT_STORAGE_ACCESS_KEY").ok(),
            secret_key: std::env::var("ARTIFACT_STORAGE_SECRET_KEY").ok(),
            region: std::env::var("ARTIFACT_STORAGE_REGION")
                .unwrap_or_else(|_| default_storage_region()),
        }
    }
}

fn default_storage_type() -> String {
    "local".to_string()
}

fn default_storage_region() -> String {
    "us-east-1".to_string()
}

/// Docker 容器执行配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerConfig {
    /// 是否启用 Docker 执行
    #[serde(default)]
    pub enabled: bool,

    /// 默认 Docker 镜像
    #[serde(default = "default_docker_image")]
    pub default_image: String,

    /// 按步骤类型指定的自定义镜像
    #[serde(default)]
    pub custom_images: HashMap<String, String>,

    /// 资源限制
    #[serde(default)]
    pub resource_limits: DockerResourceLimits,

    /// 安全配置
    #[serde(default)]
    pub security: DockerSecurityConfig,

    /// 网络模式 (bridge, host, none)
    #[serde(default)]
    pub network_mode: Option<String>,

    /// 默认超时（秒）
    #[serde(default = "default_docker_timeout")]
    pub default_timeout: u64,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_image: default_docker_image(),
            custom_images: HashMap::new(),
            resource_limits: DockerResourceLimits::default(),
            security: DockerSecurityConfig::default(),
            network_mode: None,
            default_timeout: default_docker_timeout(),
        }
    }
}

/// Docker 资源限制
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerResourceLimits {
    /// 内存限制（GB）
    #[serde(default)]
    pub memory: Option<i64>,

    /// 交换空间限制（GB）
    #[serde(default)]
    pub memory_swap: Option<i64>,

    /// CPU 配额（微秒，需要配合 period 使用）
    #[serde(default)]
    pub cpu_quota: Option<i64>,

    /// CPU 周期（微秒）
    #[serde(default)]
    pub cpu_period: Option<i64>,

    /// CPU 份额（相对权重，1024 为基准）
    #[serde(default)]
    pub cpu_shares: Option<i64>,

    /// 最大进程数
    #[serde(default)]
    pub pids_limit: Option<i64>,

    /// 文件描述符限制
    #[serde(default)]
    pub nofile: Option<i64>,

    /// 进程数限制
    #[serde(default)]
    pub nproc: Option<i64>,
}

impl Default for DockerResourceLimits {
    fn default() -> Self {
        Self {
            memory: Some(4), // 4GB
            memory_swap: None,
            cpu_quota: None,
            cpu_period: None,
            cpu_shares: Some(1024),
            pids_limit: Some(1024),
            nofile: Some(65536),
            nproc: Some(4096),
        }
    }
}

/// Docker 安全配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerSecurityConfig {
    /// 是否使用非 root 用户运行
    #[serde(default)]
    pub non_root: bool,

    /// 是否使用只读根文件系统
    #[serde(default)]
    pub read_only_rootfs: bool,

    /// 是否丢弃所有能力
    #[serde(default)]
    pub drop_all_capabilities: bool,

    /// 需要添加的能力
    #[serde(default)]
    pub add_capabilities: Vec<String>,

    /// 允许访问的设备
    #[serde(default)]
    pub allowed_devices: Vec<String>,
}

impl Default for DockerSecurityConfig {
    fn default() -> Self {
        Self {
            non_root: false,
            read_only_rootfs: false,
            drop_all_capabilities: true,
            add_capabilities: vec!["CAP_NET_BIND_SERVICE".to_string()],
            allowed_devices: Vec::new(),
        }
    }
}

impl DockerSecurityConfig {
    /// 获取需要添加的能力列表
    pub fn capabilities_to_add(&self) -> Vec<String> {
        if self.drop_all_capabilities {
            self.add_capabilities.clone()
        } else {
            Vec::new()
        }
    }
}

fn default_workspace_dir() -> String {
    "/tmp/ops-runner/workspace".to_string()
}

fn default_task_timeout() -> u64 {
    3600 // 1小时
}

fn default_step_timeout() -> u64 {
    600 // 10分钟
}

fn default_docker_image() -> String {
    "ubuntu:22.04".to_string()
}

fn default_docker_timeout() -> u64 {
    1800 // 30分钟
}

impl RunnerConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            runner: RunnerInfo {
                name: std::env::var("RUNNER_NAME").context("RUNNER_NAME must be set")?,
                capabilities: std::env::var("RUNNER_CAPABILITIES")
                    .ok()
                    .unwrap_or_else(|| "node,java,rust,frontend,other".to_string())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                docker_supported: std::env::var("RUNNER_DOCKER_SUPPORTED")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true),
                max_concurrent_jobs: std::env::var("RUNNER_MAX_CONCURRENT_JOBS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2),
                outbound_allowlist: std::env::var("RUNNER_OUTBOUND_ALLOWLIST")
                    .ok()
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
                environment: std::env::var("RUNNER_ENVIRONMENT")
                    .unwrap_or_else(|_| "dev".to_string()),
            },
            control_plane: ControlPlaneConfig {
                api_url: std::env::var("CONTROL_PLANE_API_URL")
                    .context("CONTROL_PLANE_API_URL must be set")?,
                api_key: std::env::var("RUNNER_API_KEY").context("RUNNER_API_KEY must be set")?,
                heartbeat_interval_secs: std::env::var("RUNNER_HEARTBEAT_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
            },
            message_queue: MessageQueueConfig {
                amqp_url: std::env::var("RABBITMQ_AMQP_URL")
                    .context("RABBITMQ_AMQP_URL must be set")?,
                vhost: std::env::var("RABBITMQ_VHOST")
                    .ok()
                    .unwrap_or_else(|| "/".to_string()),
                exchange: std::env::var("RABBITMQ_EXCHANGE")
                    .ok()
                    .unwrap_or_else(|| "ops.build".to_string()),
                queue_prefix: std::env::var("RABBITMQ_QUEUE_PREFIX")
                    .ok()
                    .unwrap_or_else(|| "ops-runner".to_string()),
                prefetch: std::env::var("RABBITMQ_PREFETCH")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
                queue_name_override: None,
                routing_key_pattern: None,
            },
            execution: ExecutionConfig {
                workspace_base_dir: std::env::var("RUNNER_WORKSPACE_DIR")
                    .ok()
                    .unwrap_or_else(|| "/tmp/ops-runner/workspace".to_string()),
                task_timeout_secs: std::env::var("RUNNER_TASK_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3600),
                step_timeout_secs: std::env::var("RUNNER_STEP_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(600),
                cleanup_workspace: std::env::var("RUNNER_CLEANUP_WORKSPACE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(true),
                cache_dir: std::env::var("RUNNER_CACHE_DIR").ok(),
                docker: None,
            },
            artifact_storage: ArtifactStorageConfig::from_env(),
        })
    }

    /// 应用从控制面接收的 Docker 配置
    pub fn apply_docker_config(&mut self, remote_config: common::messages::RunnerDockerConfig) {
        use std::collections::HashMap;

        let custom_images: HashMap<String, String> =
            remote_config.images_by_type.into_iter().collect();

        self.execution.docker = Some(DockerConfig {
            enabled: remote_config.enabled,
            default_image: remote_config.default_image,
            custom_images,
            resource_limits: DockerResourceLimits {
                memory: remote_config.memory_limit_gb,
                memory_swap: None,
                cpu_quota: None,
                cpu_period: None,
                cpu_shares: remote_config.cpu_shares,
                pids_limit: remote_config.pids_limit,
                nofile: Some(65536),
                nproc: Some(4096),
            },
            security: DockerSecurityConfig::default(),
            network_mode: None,
            default_timeout: remote_config.default_timeout_secs,
        });
    }

    /// 获取任务超时
    pub fn task_timeout(&self) -> Duration {
        Duration::from_secs(self.execution.task_timeout_secs)
    }

    /// 获取步骤超时
    pub fn step_timeout(&self) -> Duration {
        Duration::from_secs(self.execution.step_timeout_secs)
    }

    /// 生成队列名称
    ///
    /// 控制面在注册响应中下发的完整队列名（queue_name_override）优先，
    /// 未提供时回退到本地推导：{queue_prefix}.{runner_name}.queue
    pub fn queue_name(&self) -> String {
        if let Some(name) = self.message_queue.queue_name_override.as_deref()
            && !name.is_empty()
        {
            return name.to_string();
        }
        format!(
            "{}.{}.queue",
            self.message_queue.queue_prefix,
            self.runner.name.replace('-', "_")
        )
    }

    /// 生成广播模式 routing key
    ///
    /// 契约 = 本地推导 `build.{capability}`（定向派发另有
    /// `build.{capability}.{runner_name}`，见 routing_key_for_runner）。
    /// 控制面下发的 routing_key_pattern（如 `build.{type}[.{runner_name}]`）
    /// 是含模板语法的文档字符串，不是可执行的绑定键：历史版本曾把
    /// 半替换结果（`build.general[.{runner_name}]`）当字面量绑定，
    /// 产生永不匹配的死绑定，因此仅在注册日志中展示，不参与绑定。
    pub fn routing_key(&self, capability: &str) -> String {
        format!("build.{}", capability)
    }

    /// 生成定向 routing key（仅本 Runner 接收）
    pub fn routing_key_for_runner(&self, capability: &str) -> String {
        format!("build.{}.{}", capability, self.runner.name)
    }
}

/// 测试专用：跨模块共享的 env 修改互斥锁（mod tests 私有，需模块级再导出）
#[cfg(test)]
pub(crate) fn test_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // edition 2024 中 std::env::set_var/remove_var 为 unsafe；
    // 所有 env 访问均由 test_env_lock 串行化，满足单线程前提。
    // 其他模块的测试（如 executor 的 env 白名单测试）也必须拿同一把锁，
    // 否则并行测试互踩环境变量（曾导致 PoisonError 级联失败）。
    fn env_lock() -> &'static Mutex<()> {
        crate::config::test_env_lock()
    }


    fn create_test_config() -> RunnerConfig {
        RunnerConfig {
            runner: RunnerInfo {
                name: "test-runner".to_string(),
                capabilities: vec!["node".to_string(), "rust".to_string()],
                docker_supported: true,
                max_concurrent_jobs: 4,
                outbound_allowlist: vec!["*.crates.io".to_string()],
                environment: "test".to_string(),
            },
            control_plane: ControlPlaneConfig {
                api_url: "http://localhost:3000".to_string(),
                api_key: "test-key".to_string(),
                heartbeat_interval_secs: 60,
            },
            message_queue: MessageQueueConfig {
                amqp_url: "amqp://localhost:5672".to_string(),
                vhost: "/".to_string(),
                exchange: "ops.build".to_string(),
                queue_prefix: "test-runner".to_string(),
                prefetch: 2,
                queue_name_override: None,
                routing_key_pattern: None,
            },
            execution: ExecutionConfig {
                workspace_base_dir: "/tmp/test-workspace".to_string(),
                task_timeout_secs: 1800,
                step_timeout_secs: 300,
                cleanup_workspace: true,
                cache_dir: Some("/tmp/cache".to_string()),
                docker: None,
            },
            artifact_storage: ArtifactStorageConfig::default(),
        }
    }

    #[test]
    fn test_queue_name_generation() {
        let config = create_test_config();
        assert_eq!(config.queue_name(), "test-runner.test_runner.queue");
    }

    #[test]
    fn test_queue_name_with_dashes() {
        let mut config = create_test_config();
        config.runner.name = "my-test-runner-01".to_string();
        assert_eq!(config.queue_name(), "test-runner.my_test_runner_01.queue");
    }

    #[test]
    fn test_routing_key_generation() {
        let config = create_test_config();
        assert_eq!(config.routing_key("node"), "build.node");
        assert_eq!(config.routing_key("rust"), "build.rust");
        assert_eq!(config.routing_key("java"), "build.java");
    }

    #[test]
    fn test_task_timeout() {
        let config = create_test_config();
        assert_eq!(config.task_timeout(), Duration::from_secs(1800));
    }

    #[test]
    fn test_step_timeout() {
        let config = create_test_config();
        assert_eq!(config.step_timeout(), Duration::from_secs(300));
    }

    #[test]
    fn test_default_values() {
        let _info = RunnerInfo {
            name: "test".to_string(),
            capabilities: vec![],
            docker_supported: false,
            max_concurrent_jobs: 0,
            outbound_allowlist: vec![],
            environment: "dev".to_string(),
        };

        // 默认值测试
        assert!(default_true());
        assert_eq!(default_max_concurrent(), 2);
        assert_eq!(default_heartbeat_interval(), 30);
        assert_eq!(default_vhost(), "/");
        assert_eq!(default_exchange(), "ops.build");
        assert_eq!(default_queue_prefix(), "ops-runner");
        assert_eq!(default_prefetch(), 1);
        assert_eq!(default_workspace_dir(), "/tmp/ops-runner/workspace");
        assert_eq!(default_task_timeout(), 3600);
        assert_eq!(default_step_timeout(), 600);
    }

    #[test]
    fn test_config_serialization() {
        let config = create_test_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"name\":\"test-runner\""));
        assert!(json.contains("\"api_url\":\"http://localhost:3000\""));
        assert!(json.contains("\"amqp_url\":\"amqp://localhost:5672\""));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "runner": {
                "name": "test-runner",
                "capabilities": ["node", "rust"],
                "docker_supported": true,
                "max_concurrent_jobs": 4,
                "outbound_allowlist": ["*.crates.io"],
                "environment": "test"
            },
            "control_plane": {
                "api_url": "http://localhost:3000",
                "api_key": "test-key",
                "heartbeat_interval_secs": 60
            },
            "message_queue": {
                "amqp_url": "amqp://localhost:5672"
            },
            "execution": {
                "task_timeout_secs": 1800,
                "step_timeout_secs": 300
            }
        }"#;

        let config: RunnerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.runner.name, "test-runner");
        assert_eq!(config.runner.capabilities.len(), 2);
        assert_eq!(config.control_plane.api_url, "http://localhost:3000");
        assert_eq!(config.execution.task_timeout_secs, 1800);
    }

    #[test]
    fn test_from_env_missing_required() {
        let _guard = env_lock().lock().unwrap();
        // 清除环境变量
        unsafe { std::env::remove_var("RUNNER_NAME") };
        unsafe { std::env::remove_var("CONTROL_PLANE_API_URL") };
        unsafe { std::env::remove_var("RUNNER_API_KEY") };
        unsafe { std::env::remove_var("RABBITMQ_AMQP_URL") };

        let result = RunnerConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("RUNNER_NAME"));
    }

    #[test]
    fn test_from_env_with_valid_vars() {
        let _guard = env_lock().lock().unwrap();
        unsafe { std::env::set_var("RUNNER_NAME", "env-test-runner") };
        unsafe { std::env::set_var("CONTROL_PLANE_API_URL", "http://localhost:3000") };
        unsafe { std::env::set_var("RUNNER_API_KEY", "env-test-key") };
        unsafe { std::env::set_var("RABBITMQ_AMQP_URL", "amqp://localhost:5672") };

        let config = RunnerConfig::from_env().unwrap();
        assert_eq!(config.runner.name, "env-test-runner");
        assert_eq!(config.control_plane.api_url, "http://localhost:3000");
        assert_eq!(config.control_plane.api_key, "env-test-key");
        assert_eq!(config.message_queue.amqp_url, "amqp://localhost:5672");

        // 清理
        unsafe { std::env::remove_var("RUNNER_NAME") };
        unsafe { std::env::remove_var("CONTROL_PLANE_API_URL") };
        unsafe { std::env::remove_var("RUNNER_API_KEY") };
        unsafe { std::env::remove_var("RABBITMQ_AMQP_URL") };
    }

    #[test]
    fn test_from_env_with_optional_vars() {
        let _guard = env_lock().lock().unwrap();
        unsafe { std::env::set_var("RUNNER_NAME", "test") };
        unsafe { std::env::set_var("CONTROL_PLANE_API_URL", "http://localhost:3000") };
        unsafe { std::env::set_var("RUNNER_API_KEY", "key") };
        unsafe { std::env::set_var("RABBITMQ_AMQP_URL", "amqp://localhost:5672") };

        // 设置可选变量
        unsafe { std::env::set_var("RUNNER_CAPABILITIES", "go,python") };
        unsafe { std::env::set_var("RUNNER_MAX_CONCURRENT_JOBS", "8") };
        unsafe { std::env::set_var("RUNNER_HEARTBEAT_INTERVAL_SECS", "120") };
        unsafe { std::env::set_var("RUNNER_WORKSPACE_DIR", "/custom/workspace") };

        let config = RunnerConfig::from_env().unwrap();
        assert_eq!(config.runner.capabilities, vec!["go".to_string(), "python".to_string()]);
        assert_eq!(config.runner.max_concurrent_jobs, 8);
        assert_eq!(config.control_plane.heartbeat_interval_secs, 120);
        assert_eq!(config.execution.workspace_base_dir, "/custom/workspace");

        // 清理
        unsafe { std::env::remove_var("RUNNER_NAME") };
        unsafe { std::env::remove_var("CONTROL_PLANE_API_URL") };
        unsafe { std::env::remove_var("RUNNER_API_KEY") };
        unsafe { std::env::remove_var("RABBITMQ_AMQP_URL") };
        unsafe { std::env::remove_var("RUNNER_CAPABILITIES") };
        unsafe { std::env::remove_var("RUNNER_MAX_CONCURRENT_JOBS") };
        unsafe { std::env::remove_var("RUNNER_HEARTBEAT_INTERVAL_SECS") };
        unsafe { std::env::remove_var("RUNNER_WORKSPACE_DIR") };
    }

    #[test]
    fn test_queue_name_override_from_control_plane() {
        let mut config = create_test_config();
        config.message_queue.queue_name_override = Some("runner.my-runner.queue".to_string());
        // 控制面下发的完整队列名优先
        assert_eq!(config.queue_name(), "runner.my-runner.queue");

        config.message_queue.queue_name_override = Some(String::new());
        // 空值视为未提供，回退到本地推导
        assert_eq!(config.queue_name(), "test-runner.test_runner.queue");
    }

    #[test]
    fn test_routing_key_pattern_from_control_plane() {
        let mut config = create_test_config();
        // 控制面下发的 pattern 是文档字符串，不影响实际绑定键（见 routing_key 文档）
        config.message_queue.routing_key_pattern = Some("build.{type}[.{runner_name}]".to_string());
        assert_eq!(config.routing_key("node"), "build.node");
        assert_eq!(config.routing_key("general"), "build.general");

        // 未下发时同样本地推导
        config.message_queue.routing_key_pattern = None;
        assert_eq!(config.routing_key("node"), "build.node");
    }

    #[test]
    fn test_artifact_storage_config_defaults() {
        let config = ArtifactStorageConfig::default();
        assert_eq!(config.storage_type, "local");
        assert_eq!(config.region, "us-east-1");
        assert!(config.endpoint.is_none());
    }

    #[test]
    fn test_config_deserialization_defaults_for_new_fields() {
        // 旧配置文件缺少新字段时应能正常反序列化
        let json = r#"{
            "runner": {
                "name": "test-runner",
                "environment": "test"
            },
            "control_plane": {
                "api_url": "http://localhost:3000",
                "api_key": "test-key"
            },
            "message_queue": {
                "amqp_url": "amqp://localhost:5672"
            },
            "execution": {}
        }"#;

        let config: RunnerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.message_queue.queue_name_override, None);
        assert_eq!(config.message_queue.routing_key_pattern, None);
        assert_eq!(config.artifact_storage.storage_type, "local");
        assert_eq!(config.artifact_storage.region, "us-east-1");
    }
}
