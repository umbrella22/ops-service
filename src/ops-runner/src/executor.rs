//! 构建任务执行引擎

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::{DockerConfig, RunnerConfig};
use crate::docker::DockerExecutor;
use crate::messages::*;
use crate::publisher::{ArtifactStorage, MessagePublisher};

/// 允许传递给构建步骤和 Docker 容器的宿主环境变量白名单。
///
/// runner 进程环境中包含 RUNNER_API_KEY、带凭据的 AMQP URL 等敏感信息，
/// 出于密钥隔离考虑不能把整个进程环境透传给构建步骤或容器，
/// 因此仅按白名单挑选构建所需的通用变量，再叠加任务声明的 env_vars。
const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "TERM",
    "TZ",
    "TMPDIR",
    "DOCKER_HOST",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE_OVERRIDE",
];

/// 单个 git 操作（clone/fetch/checkout）的超时时间
const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// 子进程输出捕获上限（10 MiB），防止失控命令耗尽内存
const MAX_STEP_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// 取消记录的保留时长（TTL）：超过后同 job 的新任务（如重新派发）
/// 按正常任务执行
const CANCEL_TTL: Duration = Duration::from_secs(3600);

/// 取消后的收尾宽限期：等待运行 future 在协作式取消下完成步骤清理
/// （杀子进程 / 停止并移除容器）；宽限耗尽则丢弃运行 future 兜底
const CANCEL_GRACE: Duration = Duration::from_secs(30);

/// 清理超过 TTL 的取消记录
fn prune_cancelled(cancelled: &mut HashMap<Uuid, Instant>) {
    cancelled.retain(|_, cancelled_at| cancelled_at.elapsed() < CANCEL_TTL);
}

/// 构建步骤执行环境：宿主白名单变量 + 任务声明的 env_vars（后者覆盖同名变量）
fn build_step_envs(task_env_vars: &HashMap<String, String>) -> HashMap<String, String> {
    let host_vars: HashMap<String, String> = std::env::vars().collect();

    let mut envs: HashMap<String, String> = ENV_ALLOWLIST
        .iter()
        .filter_map(|key| host_vars.get(*key).map(|value| (key.to_string(), value.clone())))
        .collect();

    for (key, value) in task_env_vars {
        envs.insert(key.clone(), value.clone());
    }

    envs
}

/// 清理步骤 ID，仅保留 [A-Za-z0-9._-]，其余替换为 '_'，并消除 ".." 序列。
///
/// 步骤 ID 来自消息队列，可能包含路径分隔符等危险字符，
/// 直接拼进脚本文件名或 Docker 容器名会导致路径穿越。
pub(crate) fn sanitize_step_id(id: &str) -> String {
    let mut sanitized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();

    while sanitized.contains("..") {
        sanitized = sanitized.replace("..", "_");
    }

    if sanitized.is_empty() {
        "step".to_string()
    } else {
        sanitized
    }
}

/// 校验产物远端路径的单一组件：拒绝空、".."、"." 及包含路径分隔符的值
fn sanitize_path_component(component: &str) -> Result<String> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.contains('/')
        || component.contains('\\')
    {
        anyhow::bail!("Invalid path component: {:?}", component);
    }
    Ok(component.to_string())
}

/// 以 1 MiB 分块流式计算文件 SHA256，避免把大文件整体读入内存
pub(crate) async fn sha256_file_hex(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("Failed to open artifact file: {:?}", path))?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// 校验 git 仓库地址与分支名，防止参数注入。
///
/// 分支以 '-' 开头时会被 git 当作选项解析；仓库地址仅允许
/// http/https/ssh/git/file 协议及 scp 风格（git@host:path，等价于 ssh）写法，
/// 其他形式（如 ext::、本地路径）一律拒绝。
fn validate_git_inputs(repo_url: &str, branch: &str) -> Result<()> {
    if branch.is_empty() || branch.starts_with('-') || branch.contains(char::is_whitespace) {
        anyhow::bail!("Invalid branch name: {:?}", branch);
    }

    const ALLOWED_SCHEMES: [&str; 5] = ["http://", "https://", "ssh://", "git://", "file://"];
    let has_allowed_scheme = ALLOWED_SCHEMES.iter().any(|s| repo_url.starts_with(s));
    // scp 风格（git@host:path）等价于 ssh 协议
    let is_scp_like =
        !repo_url.contains("://") && repo_url.contains('@') && repo_url.contains(':');

    if repo_url.starts_with('-') || (!has_allowed_scheme && !is_scp_like) {
        anyhow::bail!("Unsupported repository URL: {:?}", repo_url);
    }

    Ok(())
}

/// 执行 git 命令（异步 + 超时保护），超时或取消时子进程随 future 一起被杀掉
async fn run_git(
    args: &[&str],
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).kill_on_drop(true);

    let output = tokio::select! {
        biased;
        // 取消：丢弃命令 future，子进程随 kill_on_drop 被杀
        _ = cancel.cancelled() => {
            anyhow::bail!("git {} aborted by cancellation", args.join(" "))
        }
        r = tokio::time::timeout(timeout, cmd.output()) => r
            .map_err(|_| anyhow::anyhow!("git {} timed out after {:?}", args.join(" "), timeout))?
            .context("Failed to execute git command")?,
    };

    Ok(output)
}

/// 持续读取管道并最多保留 cap 字节；管道不存在时返回空
async fn read_capped<R>(pipe: Option<R>, cap: usize) -> std::io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut out = Vec::new();
    let Some(mut pipe) = pipe else {
        return Ok(out);
    };
    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        if out.len() < cap {
            let take = read.min(cap - out.len());
            out.extend_from_slice(&buffer[..take]);
        }
    }

    Ok(out)
}

/// 工作空间管理器
pub struct WorkspaceManager {
    base_dir: PathBuf,
    cleanup_policy: CleanupPolicy,
    /// 最大工作空间大小（MB）
    max_size_mb: Option<usize>,
}

/// 工作空间清理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupPolicy {
    /// 执行后立即清理
    Immediate,
    /// 保留一段时间后清理（秒）
    AfterDuration(u64),
    /// 保留最近 N 个
    RetainRecent(usize),
    /// 手动清理
    Manual,
}

impl WorkspaceManager {
    /// 创建新的工作空间管理器
    #[allow(dead_code)]
    pub fn new(base_dir: String, retain_count: usize, max_size_mb: Option<usize>) -> Result<Self> {
        let base_dir = PathBuf::from(base_dir);
        fs::create_dir_all(&base_dir).context("Failed to create workspace base directory")?;

        Ok(Self {
            base_dir,
            cleanup_policy: CleanupPolicy::RetainRecent(retain_count),
            max_size_mb,
        })
    }

    /// 从配置创建
    pub fn from_config(config: &RunnerConfig) -> Result<Self> {
        let retain_count = std::env::var("WORKSPACE_RETAIN_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        let max_size_mb = std::env::var("WORKSPACE_MAX_SIZE_MB")
            .ok()
            .and_then(|v| v.parse().ok());

        // 从环境变量读取清理策略
        let cleanup_policy = match std::env::var("WORKSPACE_CLEANUP_POLICY").as_deref() {
            Ok("immediate") => CleanupPolicy::Immediate,
            Ok("manual") => CleanupPolicy::Manual,
            Ok(duration) => {
                // 尝试解析为秒数
                if let Ok(secs) = duration.parse::<u64>() {
                    CleanupPolicy::AfterDuration(secs)
                } else {
                    CleanupPolicy::RetainRecent(retain_count)
                }
            }
            _ => CleanupPolicy::RetainRecent(retain_count),
        };

        Ok(Self {
            base_dir: PathBuf::from(&config.execution.workspace_base_dir),
            cleanup_policy,
            max_size_mb,
        })
    }

    /// 设置清理策略
    #[allow(dead_code)]
    pub fn set_cleanup_policy(&mut self, policy: CleanupPolicy) {
        self.cleanup_policy = policy;
    }

    /// 获取当前清理策略
    #[allow(dead_code)]
    pub fn cleanup_policy(&self) -> CleanupPolicy {
        self.cleanup_policy
    }

    /// 创建新的工作空间
    pub fn create_workspace(&self, job_id: Uuid, task_id: Uuid) -> Result<PathBuf> {
        let workspace_name = format!("{}_{}", job_id, task_id);
        let workspace_path = self.base_dir.join(&workspace_name);

        fs::create_dir_all(&workspace_path).context("Failed to create workspace directory")?;

        info!("Created workspace: {:?}", workspace_path);
        Ok(workspace_path)
    }

    /// 清理指定工作空间（根据策略决定是否清理）
    pub fn cleanup_workspace(&self, workspace: &Path) -> Result<()> {
        if !workspace.exists() {
            return Ok(());
        }

        // 根据清理策略决定是否执行清理
        match self.cleanup_policy {
            CleanupPolicy::Manual => {
                info!("Cleanup policy is Manual, skipping workspace cleanup: {:?}", workspace);
                return Ok(());
            }
            CleanupPolicy::Immediate => {
                // 立即清理
            }
            CleanupPolicy::AfterDuration(_) | CleanupPolicy::RetainRecent(_) => {
                // 这些策略由 cleanup_old_workspaces 处理
                return Ok(());
            }
        }

        self.force_cleanup_workspace(workspace)
    }

    /// 强制清理指定工作空间（忽略策略）
    ///
    /// 大小限制仅用于告警，不改变清理行为——策略要求的清理必须执行，
    /// 否则未超限的工作空间将永远得不到清理。
    pub fn force_cleanup_workspace(&self, workspace: &Path) -> Result<()> {
        if !workspace.exists() {
            return Ok(());
        }

        // 超限告警（不影响清理决策）
        if let Some(max_mb) = self.max_size_mb
            && let Ok(size_mb) = self.get_workspace_size_mb(workspace)
            && size_mb > max_mb as u64
        {
            warn!("Workspace exceeds size limit: {} MB > {} MB", size_mb, max_mb);
        }

        // 尝试多次清理，处理文件占用问题
        for attempt in 1..=3 {
            match fs::remove_dir_all(workspace) {
                Ok(_) => {
                    info!("Cleaned up workspace: {:?}", workspace);
                    return Ok(());
                }
                Err(e) if attempt < 3 => {
                    warn!(
                        "Cleanup attempt {} failed for {:?}: {}, retrying...",
                        attempt, workspace, e
                    );
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(e) => {
                    warn!("Failed to cleanup workspace {:?}: {}", workspace, e);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    /// 清理旧工作空间（根据当前策略）
    pub fn cleanup_old_workspaces(&self) -> Result<()> {
        let entries: Vec<_> = fs::read_dir(&self.base_dir)
            .context("Failed to read workspace directory")?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().is_dir()
                    && e.path()
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.contains('_'))
                        .unwrap_or(false)
            })
            .collect();

        match self.cleanup_policy {
            CleanupPolicy::Immediate => {
                // Immediate 策略在创建工作空间后立即清理，这里不做处理
                Ok(())
            }
            CleanupPolicy::Manual => {
                // 手动清理策略，不自动清理
                Ok(())
            }
            CleanupPolicy::AfterDuration(duration_secs) => {
                // 清理超过指定时间的工作空间
                let cutoff_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    - duration_secs;

                let mut cleaned = 0;
                for entry in &entries {
                    if let Ok(metadata) = entry.metadata()
                        && let Ok(modified) = metadata.modified()
                    {
                        let modified_secs = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        if modified_secs < cutoff_time {
                            match self.force_cleanup_workspace(&entry.path()) {
                                Err(e) => {
                                    warn!(
                                        "Failed to cleanup old workspace {:?}: {}",
                                        entry.path(),
                                        e
                                    );
                                }
                                _ => {
                                    cleaned += 1;
                                }
                            }
                        }
                    }
                }
                info!("Cleaned {} workspaces older than {}s", cleaned, duration_secs);
                Ok(())
            }
            CleanupPolicy::RetainRecent(retain) => {
                if entries.len() <= retain {
                    return Ok(());
                }

                // 按修改时间排序
                let mut entries_with_time: Vec<_> = entries
                    .iter()
                    .filter_map(|e| {
                        e.metadata()
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .map(|t| (e, t))
                    })
                    .collect();

                entries_with_time.sort_by_key(|(_, t)| std::cmp::Reverse(*t));

                // 删除旧的工作空间
                let mut cleaned = 0;
                for (entry, _) in entries_with_time.iter().skip(retain) {
                    let path = entry.path();
                    match self.force_cleanup_workspace(&path) {
                        Err(e) => {
                            warn!("Failed to cleanup old workspace {:?}: {}", path, e);
                        }
                        _ => {
                            cleaned += 1;
                        }
                    }
                }

                info!("Cleaned {} old workspaces, retained {} most recent", cleaned, retain);
                Ok(())
            }
        }
    }

    /// 获取工作空间大小（MB）
    pub fn get_workspace_size_mb(&self, workspace: &Path) -> Result<u64> {
        let total_size = self.dir_size(workspace)?;
        Ok(total_size / 1024 / 1024)
    }

    /// 计算目录大小（字节）
    fn dir_size(&self, path: &Path) -> Result<u64> {
        let mut total = 0;
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    total += self.dir_size(&entry_path)?;
                } else {
                    total += entry.metadata()?.len();
                }
            }
        } else {
            total = fs::metadata(path)?.len();
        }
        Ok(total)
    }

    /// 清理所有工作空间（紧急情况）
    #[allow(dead_code)]
    pub fn cleanup_all(&self) -> Result<()> {
        let entries: Vec<_> = fs::read_dir(&self.base_dir)
            .context("Failed to read workspace directory")?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();

        let mut cleaned = 0;
        for path in entries {
            if path.is_dir() {
                match self.force_cleanup_workspace(&path) {
                    Err(e) => {
                        warn!("Failed to cleanup {:?}: {}", path, e);
                    }
                    _ => {
                        cleaned += 1;
                    }
                }
            }
        }

        info!("Cleaned up all {} workspaces", cleaned);
        Ok(())
    }
}

/// 步骤执行结果（Runner 内部使用）
///
/// 与 common::messages::StepStatus 分离：Cancelled 仅存在于 Runner 内部，
/// 表达"作业取消导致的步骤中止"。被取消的步骤不再向控制面上报步骤状态，
/// 作业终态 Cancelled 由任务级取消路径统一发布。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepOutcome {
    /// 成功
    Success,
    /// 失败
    Failed,
    /// 超时
    Timeout,
    /// 被作业取消中止
    Cancelled,
}

/// 构建执行引擎
pub struct BuildExecutor {
    /// 共享配置（运行中可被控制面热更新）
    config: Arc<RwLock<RunnerConfig>>,
    workspace_manager: WorkspaceManager,
    artifact_storage: Option<ArtifactStorage>,
    /// Docker 执行器缓存（连同其配置一起缓存，配置变化时重建）
    docker_executor: RwLock<Option<(DockerConfig, DockerExecutor)>>,
    /// 活跃任务的取消令牌（job_id -> token），取消时触发协作式取消
    ///
    /// 同 job 并发重试时以后注册者为准（控制面正常不会并发派发同一 job）；
    /// 注销时校验令牌身份，避免误删其他任务的注册。
    active_tasks: Mutex<HashMap<Uuid, Arc<CancellationToken>>>,
    /// 最近被取消的 job_id 及取消时间（TTL 内到达的迟到任务直接上报 Cancelled）
    cancelled_recently: Mutex<HashMap<Uuid, Instant>>,
}

impl BuildExecutor {
    /// 创建新的执行引擎
    pub async fn new(config: Arc<RwLock<RunnerConfig>>) -> Result<Self> {
        let snapshot = config.read().await.clone();
        let workspace_manager = WorkspaceManager::from_config(&snapshot)?;

        // 初始化产物存储
        let artifact_storage = {
            info!(
                "Artifact storage initialized: type={}",
                snapshot.artifact_storage.storage_type
            );
            Some(ArtifactStorage::from_config(&snapshot.artifact_storage))
        };

        // 启动时清理旧工作空间
        if let Err(e) = workspace_manager.cleanup_old_workspaces() {
            warn!("Failed to cleanup old workspaces on startup: {}", e);
        }

        Ok(Self {
            config,
            workspace_manager,
            artifact_storage,
            docker_executor: RwLock::new(None),
            active_tasks: Mutex::new(HashMap::new()),
            cancelled_recently: Mutex::new(HashMap::new()),
        })
    }

    /// 取消指定作业（幂等）
    ///
    /// 先记录取消意图：TTL 内迟到的同 job 任务（取消先于任务送达）会在
    /// execute() 入口直接上报 Cancelled 终态；若该 job 正在本 Runner 执行，
    /// 触发其取消令牌，正在执行的步骤在各自的取消感知点快速终止并完成清理
    /// （native 子进程 kill、Docker 容器 stop+remove）。令牌重复取消无副作用。
    pub async fn cancel_job(&self, job_id: Uuid) {
        self.mark_cancelled(job_id);

        let token = self
            .active_tasks
            .lock()
            .expect("active task registry poisoned")
            .get(&job_id)
            .cloned();

        match token {
            Some(token) => {
                info!("Cancelling active build: job={}", job_id);
                token.cancel();
            }
            None => {
                debug!(
                    "Cancel recorded for job not active on this runner: job={}",
                    job_id
                );
            }
        }
    }

    /// 记录取消意图，并顺手清理超过 TTL 的旧记录
    fn mark_cancelled(&self, job_id: Uuid) {
        let mut cancelled = self
            .cancelled_recently
            .lock()
            .expect("cancelled registry poisoned");
        prune_cancelled(&mut cancelled);
        cancelled.insert(job_id, Instant::now());
    }

    /// 检查 job 是否在 TTL 内被取消过
    fn was_recently_cancelled(&self, job_id: Uuid) -> bool {
        let mut cancelled = self
            .cancelled_recently
            .lock()
            .expect("cancelled registry poisoned");
        prune_cancelled(&mut cancelled);
        cancelled.contains_key(&job_id)
    }

    /// 注册活跃任务的取消令牌（同 job 重复注册时以后注册者为准）
    fn register_active_task(&self, job_id: Uuid, token: Arc<CancellationToken>) {
        let mut active = self
            .active_tasks
            .lock()
            .expect("active task registry poisoned");
        active.insert(job_id, token);
    }

    /// 获取（或按需初始化）与当前配置匹配的 Docker 执行器。
    ///
    /// 每次调用读取任务开始时的配置快照；Docker 配置变化时重建执行器，
    /// 使控制面下发的镜像/资源限制热更新能够生效。
    async fn try_get_docker_executor(&self, config: &RunnerConfig) -> Option<DockerExecutor> {
        if !config.runner.docker_supported {
            return None;
        }

        if !config.execution.is_docker_enabled() {
            return None;
        }

        let docker_cfg = config.execution.docker_config()?.clone();

        let mut cached = self.docker_executor.write().await;
        if let Some((cached_cfg, executor)) = cached.as_ref() {
            if *cached_cfg == docker_cfg {
                return Some(executor.clone());
            }
            info!("Docker configuration changed, re-initializing Docker executor");
        }

        match DockerExecutor::new(docker_cfg.clone()).await {
            Ok(executor) => {
                if executor.is_available() {
                    *cached = Some((docker_cfg, executor.clone()));
                    Some(executor)
                } else {
                    None
                }
            }
            Err(e) => {
                warn!("Docker executor initialization failed: {}", e);
                None
            }
        }
    }

    /// 清理资源（用于关闭时调用）
    #[allow(dead_code)]
    pub async fn cleanup(&self) -> Result<()> {
        info!("Starting executor cleanup...");

        // 清理 Docker 容器
        let cached = self.docker_executor.read().await;
        if let Some((_, executor)) = cached.as_ref()
            && let Err(e) = executor.cleanup_containers().await
        {
            warn!("Failed to cleanup Docker containers: {}", e);
        }
        drop(cached);

        // 清理旧工作空间
        if let Err(e) = self.workspace_manager.cleanup_old_workspaces() {
            warn!("Failed to cleanup old workspaces: {}", e);
        }

        info!("Executor cleanup completed");
        Ok(())
    }

    /// 执行构建任务
    ///
    /// 无论成功、失败、任务级超时还是取消，都会发布一个终态
    /// （Succeeded/Failed/Timeout/Cancelled）。返回 Ok 表示任务已到达终态且
    /// 终态已成功发布；返回 Err 表示终态未能发布，由 Worker 兜底上报。
    pub async fn execute(
        &self,
        task: BuildTaskMessage,
        publisher: &MessagePublisher,
        attempt_id: Uuid,
    ) -> Result<()> {
        info!("Starting build execution: job={}, task={}", task.job_id, task.task_id);

        // 已取消的作业直接上报 Cancelled 终态，不启动执行
        //（覆盖"取消消息先于任务消息送达"的场景）
        if self.was_recently_cancelled(task.job_id) {
            info!("Job cancelled before start: job={}", task.job_id);
            return Self::publish_cancelled(publisher, &task, attempt_id, "cancelled before start")
                .await;
        }

        // 注册取消令牌，随后复检取消记录：关闭"取消恰在检查与注册之间到达"
        // 的竞态窗口——此时令牌立即取消，下方 select 的取消分支随即生效
        let token = Arc::new(CancellationToken::new());
        self.register_active_task(task.job_id, token.clone());
        if self.was_recently_cancelled(task.job_id) {
            warn!(
                "Job cancelled while registering, cancelling immediately: job={}",
                task.job_id
            );
            token.cancel();
        }

        // 无论哪条路径退出，都先注销活跃任务再返回结果
        let result = self
            .execute_with_token(&task, publisher, attempt_id, &token)
            .await;

        // 仅当注册的仍是本任务的令牌时才移除，避免误删同 job 并发任务的令牌
        let mut active = self
            .active_tasks
            .lock()
            .expect("active task registry poisoned");
        if active
            .get(&task.job_id)
            .is_some_and(|registered| Arc::ptr_eq(registered, &token))
        {
            active.remove(&task.job_id);
        }

        result
    }

    /// 发布 Cancelled 终态；发布失败时返回 Err，由 Worker 兜底上报
    ///
    /// 取消不属于任何错误分类，error_category 置为 None。
    async fn publish_cancelled(
        publisher: &MessagePublisher,
        task: &BuildTaskMessage,
        attempt_id: Uuid,
        reason: &str,
    ) -> Result<()> {
        match publisher
            .publish_build_status(
                task,
                BuildStatus::Cancelled,
                None,
                Some(reason.to_string()),
                None,
                attempt_id,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(pe) => Err(anyhow::anyhow!(
                "{}; failed to publish cancelled status: {}",
                reason,
                pe
            )),
        }
    }

    /// 执行构建任务（已注册取消令牌）
    async fn execute_with_token(
        &self,
        task: &BuildTaskMessage,
        publisher: &MessagePublisher,
        attempt_id: Uuid,
        token: &CancellationToken,
    ) -> Result<()> {
        // 每个任务开始时读取一次配置快照，使控制面热更新生效
        let config = self.config.read().await.clone();

        // 发送接收状态
        publisher
            .publish_build_status(task, BuildStatus::Received, None, None, None, attempt_id)
            .await?;

        // 创建 workspace
        let workspace = match self.workspace_manager.create_workspace(task.job_id, task.task_id) {
            Ok(ws) => ws,
            Err(e) => {
                let msg = format!("Failed to create workspace: {:#}", e);
                // 发布 Failed 终态；发布本身失败则交由 Worker 兜底
                publisher
                    .publish_error(task, &msg, ErrorCategory::Resource, attempt_id)
                    .await?;
                return Ok(());
            }
        };

        // 发送准备中状态
        publisher
            .publish_build_status(task, BuildStatus::Preparing, None, None, None, attempt_id)
            .await?;

        // 任务级超时：优先使用任务声明的 timeout_secs，否则回退 Runner 配置
        let task_timeout = task
            .build
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or_else(|| config.task_timeout());

        // 任务超时与取消令牌同时监控执行：
        // - 令牌触发（biased 优先）时进入收尾阶段：给运行 future 一个宽限期，
        //   让正在执行的步骤在协作式取消下完成自身清理（杀子进程 / 停止并
        //   移除容器）后返回，而不是立即丢弃运行 future 导致容器清理中断；
        //   宽限耗尽（清理路径异常卡死）才丢弃运行 future 兜底，此时 native
        //   子进程随 kill_on_drop 被杀，容器可能残留、依赖 Runner 重启时的
        //   容器清理兜底；
        // - 超时触发时运行 future 被丢弃，native 子进程随 kill_on_drop 被杀。
        let run_fut = tokio::time::timeout(
            task_timeout,
            self.clone_and_run_steps(&workspace, task, &config, publisher, attempt_id, token),
        );
        tokio::pin!(run_fut);

        let cancel_fut = token.cancelled();
        tokio::pin!(cancel_fut);

        /// 任务运行结果
        enum Outcome {
            /// clone_and_run_steps 已结束（终态由其内部发布，或发布失败返回 Err）
            Completed(Result<()>),
            /// 任务级超时
            TimedOut,
            /// 作业被取消（Cancelled 终态由本层发布）
            Cancelled,
        }

        let outcome = tokio::select! {
            biased;
            _ = &mut cancel_fut => {
                match tokio::time::timeout(CANCEL_GRACE, &mut run_fut).await {
                    // 运行 future 已在协作式取消下收尾（其内部不会发布终态）
                    Ok(_) => Outcome::Cancelled,
                    Err(_) => {
                        warn!(
                            "Cancel grace period expired, aborting run: job={}",
                            task.job_id
                        );
                        Outcome::Cancelled
                    }
                }
            }
            r = &mut run_fut => match r {
                Ok(inner) => Outcome::Completed(inner),
                Err(_) => Outcome::TimedOut,
            },
        };

        // 无论成功、失败、超时还是取消，都按策略清理 workspace
        self.cleanup_workspace(&workspace).await;

        let result = match outcome {
            Outcome::Completed(inner) => inner,
            Outcome::TimedOut => {
                // 任务级超时：发布 Timeout 终态
                let msg = format!("Task timed out after {:?}", task_timeout);
                warn!(
                    "Task timed out: job={}, task={}: {}",
                    task.job_id, task.task_id, msg
                );
                match publisher
                    .publish_build_status(
                        task,
                        BuildStatus::Timeout,
                        None,
                        Some(msg.clone()),
                        Some(ErrorCategory::Timeout),
                        attempt_id,
                    )
                    .await
                {
                    Ok(_) => Ok(()),
                    // 终态发布失败，交由 Worker 兜底上报
                    Err(pe) => Err(anyhow::anyhow!(
                        "{}; failed to publish timeout status: {}",
                        msg,
                        pe
                    )),
                }
            }
            Outcome::Cancelled => {
                info!("Build cancelled: job={}, task={}", task.job_id, task.task_id);
                Self::publish_cancelled(publisher, task, attempt_id, "cancelled by user request")
                    .await
            }
        };

        // 定期清理旧工作空间
        let _ = self.workspace_manager.cleanup_old_workspaces();

        result
    }

    /// 克隆代码并执行全部构建步骤，发布最终状态（Succeeded/Failed）
    ///
    /// 作业被取消时不发布终态（返回 Ok），由任务级取消路径统一发布 Cancelled；
    /// 调用方通过令牌状态区分这两种 Ok。
    async fn clone_and_run_steps(
        &self,
        workspace: &Path,
        task: &BuildTaskMessage,
        config: &RunnerConfig,
        publisher: &MessagePublisher,
        attempt_id: Uuid,
        token: &CancellationToken,
    ) -> Result<()> {
        // 克隆代码（git 各操作均感知取消令牌）
        if let Err(e) = self.clone_code(workspace, &task.project, token).await {
            // 取消导致的克隆中断：交由任务级取消路径发布 Cancelled 终态
            if token.is_cancelled() {
                info!("Clone aborted by cancellation: job={}", task.job_id);
                return Ok(());
            }
            // 克隆失败：发布 Failed 终态（发布失败则向上返回，由 Worker 兜底）
            publisher
                .publish_error(task, &e.to_string(), ErrorCategory::Network, attempt_id)
                .await?;
            return Ok(());
        }

        // Pipeline-as-code：steps 为空时从仓库读管道文件解析步骤
        let (task, steps) = if task.steps.is_empty() {
            let file_rel = task
                .pipeline_file
                .as_deref()
                .filter(|f| !f.is_empty())
                .unwrap_or(common::pipeline::DEFAULT_PIPELINE_FILE);
            let pipeline_path = workspace.join(file_rel);
            match std::fs::read_to_string(&pipeline_path) {
                Ok(yaml) => match common::pipeline::PipelineFile::parse(&yaml) {
                    Ok(pipeline) => {
                        // 注意：不发布带伪 step_id 的日志消息——
                        // 控制面按 step_id 懒创建步骤行，伪 id 会变成假步骤
                        info!(
                            "Loaded pipeline from {}: {} step(s): {}",
                            file_rel,
                            pipeline.steps.len(),
                            pipeline
                                .steps
                                .iter()
                                .map(|s| s.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        // 步骤级默认 env：管道 env 打底，任务 env_vars 覆盖
                        let mut eff_task = task.clone();
                        let mut env = pipeline.env.clone();
                        env.extend(task.build.env_vars.clone());
                        eff_task.build.env_vars = env;
                        let steps = pipeline.to_build_steps();
                        (eff_task, steps)
                    }
                    Err(e) => {
                        let msg = format!("Pipeline file {file_rel} is invalid: {e}");
                        error!("{msg}");
                        publisher
                            .publish_error(task, &msg, ErrorCategory::Build, attempt_id)
                            .await?;
                        return Ok(());
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let msg = format!(
                        "Pipeline file {file_rel} not found in repository; nothing to build"
                    );
                    error!("{msg}");
                    publisher
                        .publish_error(task, &msg, ErrorCategory::Build, attempt_id)
                        .await?;
                    return Ok(());
                }
                Err(e) => {
                    let msg = format!("Failed to read pipeline file {file_rel}: {e}");
                    error!("{msg}");
                    publisher
                        .publish_error(task, &msg, ErrorCategory::Build, attempt_id)
                        .await?;
                    return Ok(());
                }
            }
        } else {
            (task.clone(), task.steps.clone())
        };

        // 发送执行中状态
        publisher
            .publish_build_status(&task, BuildStatus::Running, None, None, None, attempt_id)
            .await?;

        // 执行构建步骤
        let mut any_step_failed = false;

        for step in &steps {
            // 已取消的作业不再启动后续步骤
            if token.is_cancelled() {
                info!(
                    "Build cancelled, skipping remaining steps: job={}",
                    task.job_id
                );
                return Ok(());
            }

            // 步骤状态由 execute_step 内部发布（start/terminal，含
            // exit_code 与 artifact），循环层不再重复发布——重复消息
            // 会在控制面产生竞态（实测：步骤终态偶发丢失）
            match self
                .execute_step(config, workspace, &task, step, publisher, attempt_id, token)
                .await
            {
                Ok(StepOutcome::Cancelled) => {
                    warn!("Step {} aborted by cancellation", step.name);
                    // 步骤被取消：不发布失败状态，作业终态 Cancelled 由任务级路径发布
                    return Ok(());
                }
                Ok(outcome) => {
                    // 步骤终态已由 execute_step 内部发布（见上），此处仅做失败传播
                    if matches!(outcome, StepOutcome::Failed | StepOutcome::Timeout) {
                        error!("Step {} finished with status {:?}", step.name, outcome);
                        any_step_failed = true;

                        if !step.continue_on_failure {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Step {} failed: {}", step.name, e);
                    // 取消期间的错误（如状态发布被中断）按取消处理
                    if token.is_cancelled() {
                        warn!("Step {} aborted by cancellation", step.name);
                        return Ok(());
                    }
                    any_step_failed = true;

                    if !step.continue_on_failure {
                        break;
                    }
                }
            }
        }

        // 取消后不发布 Succeeded/Failed 终态，交给任务级取消路径发布 Cancelled
        if token.is_cancelled() {
            info!(
                "Build cancelled, skipping final status: job={}",
                task.job_id
            );
            return Ok(());
        }

        // 发送最终状态：任一步骤失败（含超时）即整体失败，
        // continue_on_failure=true 的失败步骤按原样上报，但不会掩盖失败
        let final_status = if any_step_failed {
            BuildStatus::Failed
        } else {
            BuildStatus::Succeeded
        };

        publisher
            .publish_build_status(&task, final_status.clone(), None, None, None, attempt_id)
            .await?;

        info!(
            "Build execution completed: job={}, task={}, status={:?}",
            task.job_id, task.task_id, final_status
        );

        Ok(())
    }

    /// 克隆代码
    ///
    /// 指定 commit 时 checkout 必须成功（必要时先 fetch 再重试），
    /// 否则产物实际来自分支 HEAD，却被标记为该 commit，导致版本不一致。
    /// 各 git 操作均感知取消令牌：取消时正在执行的 git 子进程被杀、操作中止。
    async fn clone_code(
        &self,
        workspace: &Path,
        project: &ProjectInfo,
        token: &CancellationToken,
    ) -> Result<()> {
        info!("Cloning repository: {} (branch: {})", project.repository_url, project.branch);

        // 校验输入，防止参数注入
        validate_git_inputs(&project.repository_url, &project.branch)?;

        // 处理 file:// 协议：去掉前缀使用本地路径
        let repo_url = match project.repository_url.strip_prefix("file://") {
            Some(path) => path,
            None => project.repository_url.as_str(),
        };

        let workspace_str = workspace.to_str().context("Workspace path is not valid UTF-8")?;

        let output = run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--branch",
                &project.branch,
                // "--" 之后才是仓库地址与目标目录，防止以 '-' 开头的参数被当作选项注入
                "--",
                repo_url,
                workspace_str,
            ],
            GIT_TIMEOUT,
            token,
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Git clone failed: {}", stderr);
        }

        // 检出指定 commit
        if !project.commit.is_empty() {
            if project.commit.starts_with('-') {
                anyhow::bail!("Invalid commit: {}", project.commit);
            }

            let checkout = run_git(
                &["-C", workspace_str, "checkout", &project.commit, "--"],
                GIT_TIMEOUT,
                token,
            )
            .await?;

            if !checkout.status.success() {
                // 浅克隆可能缺少该 commit，先显式 fetch 再重试一次
                let stderr = String::from_utf8_lossy(&checkout.stderr);
                info!(
                    "Checkout of commit {} failed ({}), fetching from origin",
                    project.commit,
                    stderr.trim()
                );

                let fetch = run_git(
                    &["-C", workspace_str, "fetch", "origin", &project.commit],
                    GIT_TIMEOUT,
                    token,
                )
                .await?;

                if !fetch.status.success() {
                    let stderr = String::from_utf8_lossy(&fetch.stderr);
                    anyhow::bail!("Git fetch of commit {} failed: {}", project.commit, stderr);
                }

                let retry = run_git(
                    &["-C", workspace_str, "checkout", &project.commit, "--"],
                    GIT_TIMEOUT,
                    token,
                )
                .await?;

                if !retry.status.success() {
                    let stderr = String::from_utf8_lossy(&retry.stderr);
                    anyhow::bail!("Git checkout of commit {} failed: {}", project.commit, stderr);
                }
            }
        }

        info!("Repository cloned successfully");
        Ok(())
    }

    /// 执行单个构建步骤
    ///
    /// 步骤内部感知取消令牌：native 子进程与 Docker 容器在取消时被杀/停止，
    /// 返回 StepOutcome::Cancelled，步骤状态不再上报（作业终态 Cancelled
    /// 由任务级取消路径统一发布）。
    #[allow(clippy::too_many_arguments)]
    async fn execute_step(
        &self,
        config: &RunnerConfig,
        workspace: &Path,
        task: &BuildTaskMessage,
        step: &BuildStep,
        publisher: &MessagePublisher,
        attempt_id: Uuid,
        token: &CancellationToken,
    ) -> Result<StepOutcome> {
        info!("Executing step: {}", step.name);

        let started_at = Utc::now();

        // 发送步骤开始状态
        publisher
            .publish_step_status(
                task,
                step,
                StepStatus::Running,
                started_at,
                None,
                None,
                None,
                attempt_id,
            )
            .await?;

        // 设置环境变量（宿主白名单 + 任务声明）
        let envs = build_step_envs(&task.build.env_vars);

        let docker_executor = self.try_get_docker_executor(config).await;

        if let Some(docker_executor) = docker_executor {
            match docker_executor.execute_step(step, workspace, envs, token).await {
                Ok(step_result) => {
                    // 容器步骤被取消：容器已在 docker 侧停止，返回后由调用方移除；
                    // 步骤状态不再上报，作业终态 Cancelled 由任务级取消路径发布
                    if step_result.cancelled {
                        warn!("Docker step {} aborted by cancellation", step.name);
                        return Ok(StepOutcome::Cancelled);
                    }

                    let completed_at = Utc::now();

                    if step_result.timed_out {
                        // 容器步骤超时
                        let output = if step_result.stderr.is_empty() {
                            step_result.stdout.clone()
                        } else if step_result.stdout.is_empty() {
                            step_result.stderr.clone()
                        } else {
                            format!("{}\n{}", step_result.stdout, step_result.stderr)
                        };
                        publisher
                            .publish_log(task, step, &output, LogLevel::Error, output.len() as u64, true, attempt_id)
                            .await?;

                        publisher
                            .publish_step_status(
                                task,
                                step,
                                StepStatus::Timeout,
                                started_at,
                                Some(completed_at),
                                Some(step_result.exit_code),
                                None,
                                attempt_id,
                            )
                            .await?;

                        return Ok(StepOutcome::Timeout);
                    }

                    if step_result.success {
                        // 发布标准输出
                        if !step_result.stdout.is_empty() {
                            publisher
                                .publish_log(
                                    task,
                                    step,
                                    &step_result.stdout,
                                    LogLevel::Info,
                                    step_result.stdout.len() as u64,
                                    true,
                                    attempt_id,
                                )
                                .await?;
                        }
                        // 发布标准错误（如果有）
                        if !step_result.stderr.is_empty() {
                            publisher
                                .publish_log(
                                    task,
                                    step,
                                    &step_result.stderr,
                                    LogLevel::Warn,
                                    step_result.stderr.len() as u64,
                                    true,
                                    attempt_id,
                                )
                                .await?;
                        }

                        let artifact = if step.produces_artifact {
                            self.create_and_upload_artifact(workspace, task, step, publisher, attempt_id)
                                .await?
                        } else {
                            None
                        };

                        publisher
                            .publish_step_status(
                                task,
                                step,
                                StepStatus::Succeeded,
                                started_at,
                                Some(completed_at),
                                Some(step_result.exit_code),
                                artifact,
                                attempt_id,
                            )
                            .await?;

                        return Ok(StepOutcome::Success);
                    }

                    // 失败时合并 stdout 和 stderr
                    let output = if step_result.stderr.is_empty() {
                        step_result.stdout.clone()
                    } else if step_result.stdout.is_empty() {
                        step_result.stderr.clone()
                    } else {
                        format!("{}\n{}", step_result.stdout, step_result.stderr)
                    };
                    publisher
                        .publish_log(task, step, &output, LogLevel::Error, output.len() as u64, true, attempt_id)
                        .await?;

                    publisher
                        .publish_step_status(
                            task,
                            step,
                            StepStatus::Failed,
                            started_at,
                            Some(completed_at),
                            Some(step_result.exit_code),
                            None,
                            attempt_id,
                        )
                        .await?;

                    return Ok(StepOutcome::Failed);
                }
                Err(e) => {
                    // 取消期间的容器执行失败（如镜像拉取被中断）按取消处理
                    if token.is_cancelled() {
                        warn!("Docker step {} aborted by cancellation: {}", step.name, e);
                        return Ok(StepOutcome::Cancelled);
                    }

                    let completed_at = Utc::now();
                    error!("Docker step execution failed: {}", e);
                    let error_msg = format!("Execution error: {}", e);
                    publisher
                        .publish_log(task, step, &error_msg, LogLevel::Error, error_msg.len() as u64, true, attempt_id)
                        .await?;

                    publisher
                        .publish_step_status(
                            task,
                            step,
                            StepStatus::Failed,
                            started_at,
                            Some(completed_at),
                            None,
                            None,
                            attempt_id,
                        )
                        .await?;

                    return Ok(StepOutcome::Failed);
                }
            }
        }

        // 确定执行命令（native 模式下脚本落盘）
        let command = if let Some(cmd) = &step.command {
            cmd.clone()
        } else if let Some(script) = &step.script {
            // step.id 来自外部消息，先清理再拼文件名，防止路径穿越
            let script_path = workspace.join(format!(".step_{}.sh", sanitize_step_id(&step.id)));
            fs::write(&script_path, script).context("Failed to write script file")?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
                    .context("Failed to make script executable")?;
            }

            format!("sh {}", script_path.display())
        } else {
            anyhow::bail!("Step must have either command or script");
        };

        let work_dir = if let Some(dir) = &step.working_dir {
            workspace.join(dir)
        } else {
            workspace.to_path_buf()
        };

        let timeout = step
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or_else(|| config.step_timeout());

        // 异步执行子进程：超时立即杀掉（kill_on_drop 兜底），输出捕获限制在 10 MiB
        let spawn_result = tokio::process::Command::new("sh")
            .args(["-c", &command])
            .current_dir(&work_dir)
            .envs(&envs)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let mut child = match spawn_result {
            Ok(child) => child,
            Err(e) => {
                let completed_at = Utc::now();
                error!("Command execution failed: {}", e);
                let error_msg = format!("Execution error: {}", e);
                publisher
                    .publish_log(task, step, &error_msg, LogLevel::Error, error_msg.len() as u64, true, attempt_id)
                    .await?;

                publisher
                    .publish_step_status(
                        task,
                        step,
                        StepStatus::Failed,
                        started_at,
                        Some(completed_at),
                        None,
                        None,
                        attempt_id,
                    )
                    .await?;

                return Ok(StepOutcome::Failed);
            }
        };

        // 分别在独立任务中读取 stdout/stderr，避免管道写满导致子进程阻塞
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let stdout_task = tokio::spawn(read_capped(stdout_pipe, MAX_STEP_OUTPUT_BYTES));
        let stderr_task = tokio::spawn(read_capped(stderr_pipe, MAX_STEP_OUTPUT_BYTES));

        let mut timed_out = false;
        let wait_result = tokio::select! {
            result = child.wait() => Some(result),
            _ = tokio::time::sleep(timeout) => {
                timed_out = true;
                // 超时杀掉子进程，避免孤儿进程继续占用资源
                let _ = child.kill().await;
                Some(child.wait().await)
            }
            // 取消：杀掉子进程并返回取消结果；步骤状态不再上报，
            // 作业终态 Cancelled 由任务级取消路径统一发布
            _ = token.cancelled() => {
                let _ = child.kill().await;
                None
            }
        };

        let Some(wait_result) = wait_result else {
            info!(
                "Step {} aborted by cancellation (child process killed)",
                step.name
            );
            return Ok(StepOutcome::Cancelled);
        };

        let completed_at = Utc::now();

        // 收集（受限的）输出；读取任务只会在管道 EOF 后结束，不会 panic
        let stdout_bytes = stdout_task
            .await
            .unwrap_or_else(|_| Ok(Vec::new()))
            .unwrap_or_default();
        let stderr_bytes = stderr_task
            .await
            .unwrap_or_else(|_| Ok(Vec::new()))
            .unwrap_or_default();
        let stdout = String::from_utf8_lossy(&stdout_bytes);
        let stderr = String::from_utf8_lossy(&stderr_bytes);

        if timed_out {
            warn!("Step execution timed out after {:?}", timeout);
            let timeout_msg = format!("Execution timed out after {:?}", timeout);
            publisher
                .publish_log(task, step, &timeout_msg, LogLevel::Error, timeout_msg.len() as u64, true, attempt_id)
                .await?;

            publisher
                .publish_step_status(
                    task,
                    step,
                    StepStatus::Timeout,
                    started_at,
                    Some(completed_at),
                    None,
                    None,
                    attempt_id,
                )
                .await?;

            return Ok(StepOutcome::Timeout);
        }

        match wait_result {
            Ok(exit_status) => {
                if exit_status.success() {
                    publisher
                        .publish_log(task, step, &stdout, LogLevel::Info, stdout.len() as u64, true, attempt_id)
                        .await?;

                    let artifact = if step.produces_artifact {
                        self.create_and_upload_artifact(workspace, task, step, publisher, attempt_id)
                            .await?
                    } else {
                        None
                    };

                    publisher
                        .publish_step_status(
                            task,
                            step,
                            StepStatus::Succeeded,
                            started_at,
                            Some(completed_at),
                            Some(exit_status.code().unwrap_or(0)),
                            artifact,
                            attempt_id,
                        )
                        .await?;

                    Ok(StepOutcome::Success)
                } else {
                    let log_content = format!("{}\n{}", stdout, stderr);
                    publisher
                        .publish_log(task, step, &log_content, LogLevel::Error, log_content.len() as u64, true, attempt_id)
                        .await?;

                    publisher
                        .publish_step_status(
                            task,
                            step,
                            StepStatus::Failed,
                            started_at,
                            Some(completed_at),
                            Some(exit_status.code().unwrap_or(1)),
                            None,
                            attempt_id,
                        )
                        .await?;

                    Ok(StepOutcome::Failed)
                }
            }
            Err(e) => {
                error!("Command execution failed: {}", e);
                let error_msg = format!("Execution error: {}", e);
                publisher
                    .publish_log(task, step, &error_msg, LogLevel::Error, error_msg.len() as u64, true, attempt_id)
                    .await?;

                publisher
                    .publish_step_status(
                        task,
                        step,
                        StepStatus::Failed,
                        started_at,
                        Some(completed_at),
                        None,
                        None,
                        attempt_id,
                    )
                    .await?;

                Ok(StepOutcome::Failed)
            }
        }
    }

    /// 创建并上传构建产物
    async fn create_and_upload_artifact(
        &self,
        workspace: &Path,
        task: &BuildTaskMessage,
        step: &BuildStep,
        publisher: &MessagePublisher,
        attempt_id: Uuid,
    ) -> Result<Option<BuildArtifact>> {
        // 查找可能的产物文件
        let artifact_patterns = vec!["target/release/*", "dist/*", "build/*", "*.jar", "*.zip"];

        for pattern in &artifact_patterns {
            if pattern.contains('*') {
                // 使用 glob 查找
                if let Ok(paths) = glob::glob(&workspace.join(pattern).to_string_lossy()) {
                    for path in paths.filter_map(|p| p.ok()).take(1) {
                        if let Some(artifact) = self.upload_artifact(&path, task, step).await? {
                            // 发布产物信息
                            publisher
                                .publish_artifact(task, step, &artifact, attempt_id)
                                .await?;
                            return Ok(Some(artifact));
                        }
                    }
                }
            } else {
                let pattern_path = workspace.join(pattern);
                if pattern_path.exists()
                    && let Some(artifact) = self.upload_artifact(&pattern_path, task, step).await?
                {
                    publisher
                        .publish_artifact(task, step, &artifact, attempt_id)
                        .await?;
                    return Ok(Some(artifact));
                }
            }
        }

        Ok(None)
    }

    /// 上传单个产物
    async fn upload_artifact(
        &self,
        artifact_path: &Path,
        task: &BuildTaskMessage,
        step: &BuildStep,
    ) -> Result<Option<BuildArtifact>> {
        let metadata = fs::metadata(artifact_path)?;
        let size = metadata.len();

        // 流式计算 SHA256，避免大文件整体读入内存
        let sha256 = sha256_file_hex(artifact_path).await?;

        let artifact_name = step.name.clone();
        let artifact_type = task.build.build_type.clone();
        let version = task.project.commit.clone();

        // 尝试上传到存储
        if let Some(storage) = &self.artifact_storage {
            // 远端路径各组件均来自外部消息，逐一校验防止路径穿越
            let project_name = sanitize_path_component(&task.project.name)?;
            let build_type = sanitize_path_component(&task.build.build_type)?;
            let version_component = if version.is_empty() {
                "latest".to_string()
            } else {
                sanitize_path_component(&version)?
            };
            let file_name = artifact_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .context("Artifact path has no file name")?;

            let remote_path = format!(
                "{}/{}/{}/{}",
                project_name, build_type, version_component, file_name
            );

            match storage.upload(artifact_path, &remote_path).await {
                Ok(result) => {
                    info!(
                        "Artifact uploaded: {} -> {} ({} bytes)",
                        artifact_name, result.url, result.size
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to upload artifact to storage: {}, keeping local artifact only",
                        e
                    );
                }
            }
        }

        let artifact = BuildArtifact {
            path: artifact_path.to_string_lossy().to_string(),
            name: artifact_name,
            artifact_type,
            size,
            sha256,
            version,
        };

        info!("Created artifact: {:?} ({} bytes)", artifact.name, artifact.size);
        Ok(Some(artifact))
    }

    /// 清理工作空间
    async fn cleanup_workspace(&self, workspace: &Path) {
        let config = self.config.read().await;
        if config.execution.cleanup_workspace {
            drop(config);
            if let Err(e) = self.workspace_manager.cleanup_workspace(workspace) {
                warn!("Failed to cleanup workspace {:?}: {}", workspace, e);
            }
        } else {
            info!("Workspace cleanup disabled, keeping: {:?}", workspace);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_config() -> RunnerConfig {
        RunnerConfig {
            runner: crate::config::RunnerInfo {
                name: "test-runner".to_string(),
                capabilities: vec![],
                docker_supported: false,
                max_concurrent_jobs: 1,
                outbound_allowlist: vec![],
                environment: "test".to_string(),
            },
            control_plane: crate::config::ControlPlaneConfig {
                api_url: "http://localhost:3000".to_string(),
                api_key: "test-key".to_string(),
                heartbeat_interval_secs: 30,
            },
            message_queue: crate::config::MessageQueueConfig {
                amqp_url: "amqp://localhost:5672".to_string(),
                vhost: "/".to_string(),
                exchange: "ops.build".to_string(),
                queue_prefix: "test-runner".to_string(),
                prefetch: 1,
                queue_name_override: None,
                routing_key_pattern: None,
            },
            execution: crate::config::ExecutionConfig {
                workspace_base_dir: "/tmp/ops-runner-test-workspace".to_string(),
                task_timeout_secs: 1800,
                step_timeout_secs: 300,
                cleanup_workspace: true,
                cache_dir: None,
                docker: None,
            },
            artifact_storage: crate::config::ArtifactStorageConfig::default(),
        }
    }

    #[test]
    fn test_workspace_manager_creation() {
        let temp_dir = "/tmp/test-workspace-manager";
        let manager = WorkspaceManager::new(temp_dir.to_string(), 3, Some(1000)).unwrap();

        assert_eq!(manager.base_dir, PathBuf::from(temp_dir));
        assert_eq!(manager.cleanup_policy(), CleanupPolicy::RetainRecent(3));
    }

    #[test]
    fn test_workspace_create() {
        let manager =
            WorkspaceManager::new("/tmp/test-workspace-create".to_string(), 3, None).unwrap();

        let job_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let workspace = manager.create_workspace(job_id, task_id).unwrap();

        assert!(workspace.exists());
        assert!(workspace.is_dir());

        // 清理
        let _ = fs::remove_dir_all(workspace);
        let _ = fs::remove_dir_all("/tmp/test-workspace-create");
    }

    #[test]
    fn test_workspace_cleanup() {
        let manager =
            WorkspaceManager::new("/tmp/test-workspace-cleanup".to_string(), 3, None).unwrap();

        let job_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let workspace = manager.create_workspace(job_id, task_id).unwrap();

        assert!(workspace.exists());

        manager.force_cleanup_workspace(&workspace).unwrap();

        assert!(!workspace.exists());

        let _ = fs::remove_dir_all("/tmp/test-workspace-cleanup");
    }

    #[test]
    fn test_workspace_cleanup_always_performs() {
        // 修复后：大小限制只用于告警，未超限的工作空间也必须被清理
        let manager =
            WorkspaceManager::new("/tmp/test-workspace-force".to_string(), 3, Some(1)).unwrap();

        let workspace = manager.create_workspace(Uuid::new_v4(), Uuid::new_v4()).unwrap();
        // 写入少量文件（远小于 1 MB 限制）
        fs::write(workspace.join("small.txt"), "hello").unwrap();

        manager.force_cleanup_workspace(&workspace).unwrap();
        assert!(!workspace.exists());

        let _ = fs::remove_dir_all("/tmp/test-workspace-force");
    }

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
    }

    #[tokio::test]
    async fn test_executor_creation() {
        let config = Arc::new(RwLock::new(create_test_config()));
        let executor = BuildExecutor::new(config).await;

        assert!(executor.is_ok());
        let executor = executor.unwrap();
        assert!(executor.artifact_storage.is_some());
    }

    #[test]
    fn test_cleanup_policy() {
        let policies = vec![
            CleanupPolicy::Immediate,
            CleanupPolicy::AfterDuration(3600),
            CleanupPolicy::RetainRecent(5),
            CleanupPolicy::Manual,
        ];

        for policy in policies {
            match policy {
                CleanupPolicy::Immediate => assert!(matches!(policy, CleanupPolicy::Immediate)),
                CleanupPolicy::AfterDuration(d) => assert_eq!(d, 3600),
                CleanupPolicy::RetainRecent(n) => assert_eq!(n, 5),
                CleanupPolicy::Manual => assert!(matches!(policy, CleanupPolicy::Manual)),
            }
        }
    }

    #[test]
    fn test_sanitize_step_id() {
        // 合法字符保留
        assert_eq!(sanitize_step_id("step-1_a.B"), "step-1_a.B");
        // 路径分隔符被替换
        assert_eq!(sanitize_step_id("../etc/passwd"), "__etc_passwd");
        assert_eq!(sanitize_step_id("a/b\\c"), "a_b_c");
        // ".." 序列被消除
        assert_eq!(sanitize_step_id("a..b"), "a_b");
        assert_eq!(sanitize_step_id(".."), "_");
        // 空值兜底
        assert_eq!(sanitize_step_id(""), "step");
        // 全部为非法字符时仅剩占位下划线，仍是安全文件名
        assert_eq!(sanitize_step_id("///"), "___");
    }

    #[test]
    fn test_validate_git_inputs() {
        // 合法输入
        assert!(validate_git_inputs("https://github.com/u/r.git", "main").is_ok());
        assert!(validate_git_inputs("http://git.example.com/r.git", "release-1.0").is_ok());
        assert!(validate_git_inputs("ssh://git@host/user/repo.git", "dev").is_ok());
        assert!(validate_git_inputs("git://host/repo.git", "dev").is_ok());
        assert!(validate_git_inputs("file:///tmp/repo", "main").is_ok());
        // scp 风格等价 ssh
        assert!(validate_git_inputs("git@github.com:user/repo.git", "main").is_ok());

        // 分支注入
        assert!(validate_git_inputs("https://github.com/u/r.git", "--upload-pack=evil").is_err());
        assert!(validate_git_inputs("https://github.com/u/r.git", "").is_err());
        assert!(validate_git_inputs("https://github.com/u/r.git", "a b").is_err());

        // 非法协议 / 注入
        assert!(validate_git_inputs("ext::sh -c touch /tmp/pwned", "main").is_err());
        assert!(validate_git_inputs("-oProxyCommand=evil", "main").is_err());
        assert!(validate_git_inputs("/etc/passwd", "main").is_err());
        assert!(validate_git_inputs("ftp://host/repo.git", "main").is_err());
    }

    #[test]
    fn test_sanitize_path_component() {
        assert!(sanitize_path_component("my-project").is_ok());
        assert!(sanitize_path_component("").is_err());
        assert!(sanitize_path_component(".").is_err());
        assert!(sanitize_path_component("..").is_err());
        assert!(sanitize_path_component("a/b").is_err());
        assert!(sanitize_path_component("a\\b").is_err());
    }

    #[tokio::test]
    async fn test_sha256_file_hex_streaming() {
        let dir = std::env::temp_dir().join("ops-runner-sha256-test");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("artifact.bin");

        let content = vec![42u8; 5 * 1024 * 1024 + 7]; // 跨多个 1 MiB 块
        fs::write(&file_path, &content).unwrap();

        let hash = sha256_file_hex(&file_path).await.unwrap();

        let mut expected = Sha256::new();
        expected.update(&content);
        assert_eq!(hash, hex::encode(expected.finalize()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_step_envs_allowlist() {
            let _env_guard = crate::config::test_env_lock().lock().unwrap();
        // 设置一个敏感变量模拟 runner 进程环境
        unsafe {
            std::env::set_var("RUNNER_API_KEY", "super-secret");
            std::env::set_var("OPS_RUNNER_ENV_INJECT_TEST", "leaked");
        }

        let mut task_env_vars = HashMap::new();
        task_env_vars.insert("NODE_ENV".to_string(), "test".to_string());

        let envs = build_step_envs(&task_env_vars);

        // 敏感变量不得透传
        assert!(!envs.contains_key("RUNNER_API_KEY"));
        assert!(!envs.contains_key("OPS_RUNNER_ENV_INJECT_TEST"));

        // 任务声明的变量必须存在
        assert_eq!(envs.get("NODE_ENV").map(String::as_str), Some("test"));

        unsafe {
            std::env::remove_var("RUNNER_API_KEY");
            std::env::remove_var("OPS_RUNNER_ENV_INJECT_TEST");
        }
    }

    #[test]
    fn test_cancelled_recently_prune() {
        let mut cancelled: HashMap<Uuid, Instant> = HashMap::new();
        let fresh = Uuid::new_v4();
        let stale = Uuid::new_v4();

        cancelled.insert(fresh, Instant::now());
        // 回拨超过 TTL 的时间，模拟过期记录
        cancelled.insert(stale, Instant::now() - CANCEL_TTL - Duration::from_secs(1));

        prune_cancelled(&mut cancelled);

        assert!(!cancelled.contains_key(&stale));
        assert!(cancelled.contains_key(&fresh));
    }

    #[tokio::test]
    async fn test_cancel_job_unknown_job() {
        let config = Arc::new(RwLock::new(create_test_config()));
        let executor = BuildExecutor::new(config).await.unwrap();

        let job_id = Uuid::new_v4();
        // 未知 job：不 panic，仅记录取消意图（用于关闭"取消先于任务送达"的竞态）
        executor.cancel_job(job_id).await;

        assert!(executor.was_recently_cancelled(job_id));
        // 其他 job 不受影响
        assert!(!executor.was_recently_cancelled(Uuid::new_v4()));
    }

    #[tokio::test]
    async fn test_cancel_job_cancels_active_token() {
        let config = Arc::new(RwLock::new(create_test_config()));
        let executor = BuildExecutor::new(config).await.unwrap();

        let job_id = Uuid::new_v4();
        let token = Arc::new(CancellationToken::new());
        executor.register_active_task(job_id, token.clone());
        assert!(!token.is_cancelled());

        executor.cancel_job(job_id).await;
        assert!(token.is_cancelled());

        // 幂等：重复取消不 panic
        executor.cancel_job(job_id).await;
        assert!(token.is_cancelled());
    }
}
