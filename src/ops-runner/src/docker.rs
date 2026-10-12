//! Docker 容器化执行环境
//!
//! 提供在 Docker 容器中执行构建任务的能力，包括：
//! - 容器生命周期管理（创建、启动、停止、删除）
//! - 日志流收集
//! - 资源限制（CPU、内存）
//! - 卷挂载和工作目录映射
//! - 环境变量注入

#![allow(deprecated)]

use anyhow::{Context, Result, anyhow};
use bollard::{
    Docker,
    container::LogOutput,
    models::{
        ContainerCreateBody as ContainerConfig, HostConfig, Mount, MountType, ResourcesUlimits,
    },
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, ListContainersOptions, LogsOptions,
        RemoveContainerOptions, StartContainerOptions, StopContainerOptions, WaitContainerOptions,
    },
};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::config::{DockerConfig, ExecutionConfig};
use crate::executor::sanitize_step_id;
use crate::messages::{BuildStep, StepType};

/// 容器输出收集上限（10 MiB），超过后停止累积并追加一次截断标记
const MAX_CONTAINER_OUTPUT: usize = 10 * 1024 * 1024;

/// 输出截断标记
const TRUNCATION_MARKER: &str = "\n...[output truncated]\n";

/// Docker 容器执行器
#[derive(Clone)]
pub struct DockerExecutor {
    /// Docker 客户端
    docker: Docker,
    /// Docker 配置
    config: DockerConfig,
    /// 是否连接到 Docker
    is_connected: bool,
}

impl DockerExecutor {
    /// 创建新的 Docker 执行器
    pub async fn new(config: DockerConfig) -> Result<Self> {
        // 尝试连接到 Docker
        let docker = match Docker::connect_with_local_defaults() {
            Ok(client) => {
                info!("Connected to Docker daemon");
                client
            }
            Err(e) => {
                if config.enabled {
                    // 如果启用 Docker 但连接失败，返回错误
                    return Err(anyhow!("Docker is enabled but failed to connect: {}", e));
                }
                // 如果未启用，创建一个未连接的执行器
                warn!("Docker not available, running in native mode");
                return Ok(Self {
                    docker: Docker::connect_with_local_defaults()?,
                    config,
                    is_connected: false,
                });
            }
        };

        // 验证 Docker 连接
        let _version = docker
            .version()
            .await
            .context("Failed to get Docker version")?;

        info!("Docker executor initialized successfully");

        Ok(Self {
            docker,
            config,
            is_connected: true,
        })
    }

    /// 检查 Docker 是否可用
    pub fn is_available(&self) -> bool {
        self.is_connected && self.config.enabled
    }

    /// 拉取 Docker 镜像
    pub async fn pull_image(&self, image: &str) -> Result<()> {
        info!("Pulling Docker image: {}", image);

        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);

        while let Some(next) = stream.next().await {
            match next {
                Ok(progress) => {
                    if let Some(status) = progress.status {
                        debug!("Pull progress: {}", status);
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Failed to pull image: {}", e));
                }
            }
        }

        info!("Successfully pulled image: {}", image);
        Ok(())
    }

    /// 为构建步骤获取镜像名称
    fn get_image_for_step(&self, step: &BuildStep) -> String {
        // 首先检查步骤是否指定了镜像
        if let Some(ref image) = step.docker_image {
            return image.clone();
        }

        // 检查是否为步骤类型配置了自定义镜像
        let step_type_str = match &step.step_type {
            StepType::Custom(s) => s.clone(),
            StepType::Command => "command".to_string(),
            StepType::Script => "script".to_string(),
            StepType::Install => "install".to_string(),
            StepType::Build => "build".to_string(),
            StepType::Test => "test".to_string(),
            StepType::Package => "package".to_string(),
            StepType::Publish => "publish".to_string(),
        };

        if let Some(image) = self.config.custom_images.get(&step_type_str) {
            return image.clone();
        }

        // 使用默认镜像
        self.config.default_image.clone()
    }

    /// 执行单个构建步骤
    ///
    /// 容器创建成功后，无论启动/等待/收集日志是否成功都会尽力移除容器，
    /// 避免失败路径上容器泄漏。作业取消（cancel 触发）时先停止容器并返回
    /// cancelled 结果，随后同样走移除路径。
    pub async fn execute_step(
        &self,
        step: &BuildStep,
        workspace_dir: &Path,
        env_vars: HashMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<StepResult> {
        let image = self.get_image_for_step(step);

        // 确保镜像存在
        self.pull_image(&image).await?;

        // 准备容器配置（step.id 来自外部消息，先清理再拼容器名）
        let container_name = format!(
            "ops-runner-{}-{}",
            sanitize_step_id(&step.id),
            uuid::Uuid::new_v4()
        );

        // 构建命令
        let command = self.build_command(step)?;

        // 准备环境变量
        let container_env = self.build_environment(env_vars);

        // 准备卷挂载
        let _mounts = self.build_mounts(workspace_dir);

        let working_dir = if let Some(dir) = &step.working_dir {
            let dir = dir.trim().trim_start_matches('/');
            if dir.split('/').any(|p| p == "..") {
                return Err(anyhow!("Invalid working_dir: {}", dir));
            }
            if dir.is_empty() {
                "/workspace".to_string()
            } else {
                format!("/workspace/{}", dir)
            }
        } else {
            "/workspace".to_string()
        };

        // 创建容器配置
        let host_config = HostConfig {
            binds: Some(vec![format!("{}:/workspace:rw", workspace_dir.display())]),
            // 资源限制
            memory: self
                .config
                .resource_limits
                .memory
                .map(|m| m * 1024 * 1024 * 1024),
            memory_swap: self
                .config
                .resource_limits
                .memory_swap
                .map(|m| m * 1024 * 1024 * 1024),
            cpu_quota: self.config.resource_limits.cpu_quota,
            cpu_period: self.config.resource_limits.cpu_period,
            cpu_shares: self.config.resource_limits.cpu_shares,
            pids_limit: self.config.resource_limits.pids_limit,
            network_mode: Some(
                self.config
                    .network_mode
                    .clone()
                    .unwrap_or_else(|| "bridge".to_string()),
            ),
            // 安全选项
            readonly_rootfs: Some(self.config.security.read_only_rootfs),
            cap_drop: if self.config.security.drop_all_capabilities {
                Some(vec!["ALL".to_string()])
            } else {
                None
            },
            cap_add: {
                let caps = self.config.security.capabilities_to_add();
                if caps.is_empty() { None } else { Some(caps) }
            },
            ulimits: self.build_ulimits(),
            ..Default::default()
        };

        let config = ContainerConfig {
            image: Some(image.clone()),
            cmd: Some(command),
            env: Some(container_env),
            working_dir: Some(working_dir),
            host_config: Some(host_config),
            // 非 root 用户
            user: if self.config.security.non_root {
                Some("ops-runner".to_string())
            } else {
                None
            },
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(false),
            tty: Some(false),
            ..Default::default()
        };

        // 创建容器选项
        let create_options = CreateContainerOptions {
            name: Some(container_name.clone()),
            ..Default::default()
        };

        info!("Creating container: {}", container_name);

        // 创建容器
        let _container = self
            .docker
            .create_container(Some(create_options), config)
            .await
            .context("Failed to create container")?;

        // 启动、收集日志并等待退出（受超时与作业取消约束）
        let timeout = step.timeout_secs.unwrap_or(self.config.default_timeout);
        let result = self.start_and_collect(&container_name, timeout, cancel).await;

        // 无论成功、失败、超时还是取消都移除容器，避免泄漏
        self.remove_container(&container_name).await;

        result
    }

    /// 启动容器，在超时与作业取消约束下收集日志并等待退出
    ///
    /// 使用 select! 同时等待日志流、容器退出、超时与作业取消：即使容器完全
    /// 静默（不产生任何日志）超时也能触发，随后停止并移除容器；作业取消时
    /// 立即停止容器（随后由调用方强制移除），返回取消结果。
    async fn start_and_collect(
        &self,
        container_name: &str,
        timeout_secs: u64,
        cancel: &CancellationToken,
    ) -> Result<StepResult> {
        info!("Starting container: {}", container_name);

        // 启动容器
        self.docker
            .start_container(container_name, None::<StartContainerOptions>)
            .await
            .context("Failed to start container")?;

        let deadline = tokio::time::sleep(Duration::from_secs(timeout_secs));
        tokio::pin!(deadline);

        let log_fut = self.collect_logs(container_name);
        let wait_fut = self.wait_for_container(container_name);
        tokio::pin!(log_fut, wait_fut);

        /// select! 各分支的等待结果
        enum Phase {
            Logs(Result<String>),
            Wait(i32),
            TimedOut,
            Cancelled,
        }

        let phase = tokio::select! {
            res = &mut log_fut => Phase::Logs(res),
            res = &mut wait_fut => Phase::Wait(res?),
            _ = &mut deadline => Phase::TimedOut,
            // 作业取消：停止容器（t=0 直接终止），日志流随之结束
            _ = cancel.cancelled() => Phase::Cancelled,
        };

        match phase {
            Phase::Cancelled => {
                warn!(
                    "Container step cancelled, stopping container: {}",
                    container_name
                );
                let _ = self
                    .docker
                    .stop_container(
                        container_name,
                        Some(StopContainerOptions {
                            t: Some(0),
                            signal: None,
                        }),
                    )
                    .await;

                // 停止后日志流会自然结束，收集已有输出供排查
                let logs = match log_fut.await {
                    Ok(logs) => logs,
                    Err(e) => {
                        warn!("Failed to collect logs: {}", e);
                        String::new()
                    }
                };

                Ok(StepResult {
                    exit_code: -1,
                    stdout: logs,
                    stderr: "Execution cancelled".to_string(),
                    success: false,
                    timed_out: false,
                    cancelled: true,
                })
            }
            Phase::TimedOut => {
                warn!(
                    "Container step timed out after {}s, stopping container: {}",
                    timeout_secs, container_name
                );
                let _ = self
                    .docker
                    .stop_container(
                        container_name,
                        Some(StopContainerOptions {
                            t: Some(10),
                            signal: None,
                        }),
                    )
                    .await;

                // 停止后日志流会自然结束，收集已有输出
                let logs = match log_fut.await {
                    Ok(logs) => logs,
                    Err(e) => {
                        warn!("Failed to collect logs: {}", e);
                        String::new()
                    }
                };

                Ok(StepResult {
                    exit_code: -1,
                    stdout: logs,
                    stderr: format!("Execution timed out after {}s", timeout_secs),
                    success: false,
                    timed_out: true,
                    cancelled: false,
                })
            }
            Phase::Wait(exit_code) => {
                // 容器已退出，日志流随之结束
                let logs = match log_fut.await {
                    Ok(logs) => logs,
                    Err(e) => {
                        warn!("Failed to collect logs: {}", e);
                        String::new()
                    }
                };

                info!(
                    "Step completed: {}, exit_code: {}, output_size: {} bytes",
                    container_name,
                    exit_code,
                    logs.len()
                );

                Ok(StepResult {
                    exit_code,
                    stdout: logs,
                    stderr: String::new(),
                    success: exit_code == 0,
                    timed_out: false,
                    cancelled: false,
                })
            }
            Phase::Logs(res) => {
                let logs = match res {
                    Ok(logs) => logs,
                    Err(e) => {
                        warn!("Failed to collect logs: {}", e);
                        String::new()
                    }
                };

                // 日志流结束（通常容器已退出），等待退出码
                let exit_code = wait_fut.await?;

                info!(
                    "Step completed: {}, exit_code: {}, output_size: {} bytes",
                    container_name,
                    exit_code,
                    logs.len()
                );

                Ok(StepResult {
                    exit_code,
                    stdout: logs,
                    stderr: String::new(),
                    success: exit_code == 0,
                    timed_out: false,
                    cancelled: false,
                })
            }
        }
    }

    /// 构建容器命令
    fn build_command(&self, step: &BuildStep) -> Result<Vec<String>> {
        let cmd = step
            .command
            .as_ref()
            .or(step.script.as_ref())
            .ok_or_else(|| anyhow!("Step {} has no command or script", step.id))?;

        match &step.step_type {
            StepType::Script => {
                if cfg!(windows) {
                    Ok(vec!["cmd".to_string(), "/C".to_string(), cmd.clone()])
                } else {
                    Ok(vec!["sh".to_string(), "-c".to_string(), cmd.clone()])
                }
            }
            StepType::Custom(s) if s == "bash" => {
                Ok(vec!["bash".to_string(), "-c".to_string(), cmd.clone()])
            }
            StepType::Custom(s) if s == "powershell" || s == "ps" => {
                Ok(vec!["pwsh".to_string(), "-Command".to_string(), cmd.clone()])
            }
            StepType::Custom(s) if s == "shell" || s == "sh" => {
                Ok(vec!["sh".to_string(), "-c".to_string(), cmd.clone()])
            }
            _ => {
                // 默认使用 sh -c
                Ok(vec!["sh".to_string(), "-c".to_string(), cmd.clone()])
            }
        }
    }

    /// 构建环境变量列表
    fn build_environment(&self, vars: HashMap<String, String>) -> Vec<String> {
        vars.into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }

    /// 构建 Docker 挂载点
    fn build_mounts(&self, workspace_dir: &Path) -> Vec<Mount> {
        vec![Mount {
            typ: Some(MountType::BIND),
            source: Some(workspace_dir.to_string_lossy().to_string()),
            target: Some("/workspace".to_string()),
            read_only: Some(false),
            ..Default::default()
        }]
    }

    /// 构建 ulimit 配置
    fn build_ulimits(&self) -> Option<Vec<ResourcesUlimits>> {
        let mut ulimits = Vec::new();

        // 设置文件描述符限制
        if let Some(nofile) = self.config.resource_limits.nofile {
            ulimits.push(ResourcesUlimits {
                name: Some("nofile".to_string()),
                soft: Some(nofile),
                hard: Some(nofile),
            });
        }

        // 设置进程数限制
        if let Some(nproc) = self.config.resource_limits.nproc {
            ulimits.push(ResourcesUlimits {
                name: Some("nproc".to_string()),
                soft: Some(nproc),
                hard: Some(nproc),
            });
        }

        if ulimits.is_empty() {
            None
        } else {
            Some(ulimits)
        }
    }

    /// 收集容器日志（输出上限 10 MiB，超出后追加一次截断标记）
    async fn collect_logs(&self, container_name: &str) -> Result<String> {
        let options = Some(LogsOptions {
            stdout: true,
            stderr: true,
            follow: true,
            tail: "all".to_string(),
            ..Default::default()
        });

        let stream = self.docker.logs(container_name, options);
        let mut output = String::new();
        let mut truncated = false;
        let mut stream = Box::pin(stream);

        while let Some(result) = stream.next().await {
            match result {
                Ok(log_bytes) => {
                    // LogOutput 转换为字符串
                    match log_bytes {
                        LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                            if truncated {
                                continue;
                            }
                            let text = String::from_utf8_lossy(&message);
                            if output.len() + text.len() > MAX_CONTAINER_OUTPUT {
                                // 截断到 UTF-8 字符边界，避免残缺字符
                                let mut space = MAX_CONTAINER_OUTPUT - output.len();
                                while space > 0 && !text.is_char_boundary(space) {
                                    space -= 1;
                                }
                                output.push_str(&text[..space]);
                                output.push_str(TRUNCATION_MARKER);
                                truncated = true;
                            } else {
                                output.push_str(&text);
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    warn!("Error reading log: {}", e);
                    break;
                }
            }
        }

        Ok(output)
    }

    /// 等待容器退出并返回退出码
    async fn wait_for_container(&self, container_name: &str) -> Result<i32> {
        let options = Some(WaitContainerOptions {
            condition: "not-running".to_string(),
        });

        let mut stream = self.docker.wait_container(container_name, options);

        // 多次尝试获取退出码（Docker API 偶有瞬时错误）
        for attempt in 0..5 {
            match stream.next().await {
                Some(Ok(exit)) => return Ok(exit.status_code as i32),
                Some(Err(e)) => {
                    warn!(
                        "Failed to get container exit code (attempt {}): {} — retrying",
                        attempt + 1,
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                None => {
                    warn!("Container wait stream ended without result");
                    break;
                }
            }
        }

        // 兜底：直接检查容器状态
        match self.docker.inspect_container(container_name, None).await {
            Ok(info) => {
                if let Some(state) = info.state
                    && state.running == Some(false)
                    && state.exit_code.is_some()
                {
                    return Ok(state.exit_code.unwrap_or(-1) as i32);
                }
            }
            Err(e) => warn!("Failed to inspect container {}: {}", container_name, e),
        }

        Ok(-1)
    }

    /// 移除容器
    async fn remove_container(&self, container_name: &str) {
        let options = Some(RemoveContainerOptions {
            force: true,
            v: true, // 移除关联的卷
            link: false,
        });

        match self.docker.remove_container(container_name, options).await {
            Ok(_) => debug!("Removed container: {}", container_name),
            Err(e) => warn!("Failed to remove container {}: {}", container_name, e),
        }
    }

    /// 清理所有 ops-runner 相关的容器
    #[allow(dead_code)]
    pub async fn cleanup_containers(&self) -> Result<()> {
        let containers = self
            .docker
            .list_containers(None::<ListContainersOptions>)
            .await
            .context("Failed to list containers")?;

        for container in containers {
            if let Some(name) = container.names
                && name.iter().any(|n| n.starts_with("/ops-runner-"))
                && let Some(id) = container.id
            {
                self.remove_container(&id).await;
            }
        }

        Ok(())
    }
}

/// 步骤执行结果
#[derive(Debug, Clone)]
pub struct StepResult {
    /// 退出码
    pub exit_code: i32,
    /// 标准输出
    pub stdout: String,
    /// 标准错误
    pub stderr: String,
    /// 是否成功
    pub success: bool,
    /// 是否因超时而终止
    pub timed_out: bool,
    /// 是否因作业取消而终止
    pub cancelled: bool,
}

/// 为 ExecutionConfig 添加 Docker 配置
impl ExecutionConfig {
    /// 获取 Docker 配置的引用
    pub fn docker_config(&self) -> Option<&DockerConfig> {
        self.docker.as_ref()
    }

    /// 是否启用 Docker
    pub fn is_docker_enabled(&self) -> bool {
        self.docker.as_ref().map(|d| d.enabled).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DockerResourceLimits, DockerSecurityConfig};

    #[test]
    fn test_docker_config_default() {
        let config = DockerConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.default_image, "ubuntu:22.04");
        assert_eq!(config.default_timeout, 1800);
    }

    #[test]
    fn test_docker_config_with_custom_images() {
        let mut config = DockerConfig::default();
        config
            .custom_images
            .insert("rust".to_string(), "rust:1.75".to_string());
        config
            .custom_images
            .insert("node".to_string(), "node:20".to_string());

        assert_eq!(config.custom_images.len(), 2);
        assert_eq!(config.custom_images.get("rust"), Some(&"rust:1.75".to_string()));
        assert_eq!(config.custom_images.get("node"), Some(&"node:20".to_string()));
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = DockerResourceLimits::default();
        assert_eq!(limits.memory, Some(4));
        assert_eq!(limits.cpu_shares, Some(1024));
        assert_eq!(limits.pids_limit, Some(1024));
        assert_eq!(limits.nofile, Some(65536));
        assert_eq!(limits.nproc, Some(4096));
    }

    #[test]
    fn test_security_config_default() {
        let security = DockerSecurityConfig::default();
        assert!(!security.non_root);
        assert!(!security.read_only_rootfs);
        assert!(security.drop_all_capabilities);
        assert_eq!(security.add_capabilities, vec!["CAP_NET_BIND_SERVICE"]);
    }

    #[test]
    fn test_security_config_capabilities_to_add() {
        let security = DockerSecurityConfig::default();
        let caps = security.capabilities_to_add();
        assert_eq!(caps, vec!["CAP_NET_BIND_SERVICE"]);
    }

    #[test]
    fn test_security_config_without_drop_all() {
        let security = DockerSecurityConfig {
            drop_all_capabilities: false,
            ..Default::default()
        };
        assert!(security.capabilities_to_add().is_empty());
    }

    #[test]
    fn test_step_result() {
        let result = StepResult {
            exit_code: 0,
            stdout: "hello world".to_string(),
            stderr: String::new(),
            success: true,
            timed_out: false,
            cancelled: false,
        };

        assert!(result.success);
        assert!(!result.timed_out);
        assert!(!result.cancelled);
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world");
    }

    #[test]
    fn test_build_environment() {
        let mut vars = HashMap::new();
        vars.insert("PATH".to_string(), "/usr/bin".to_string());
        vars.insert("HOME".to_string(), "/root".to_string());

        let env: Vec<String> = vars
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        assert_eq!(env.len(), 2);
        assert!(env.contains(&"PATH=/usr/bin".to_string()));
        assert!(env.contains(&"HOME=/root".to_string()));
    }
}
