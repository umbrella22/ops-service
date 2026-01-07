//! Job domain models
//! P2 阶段：作业系统（SSH 执行）

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

/// 作业类型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_type", rename_all = "snake_case")]
pub enum JobType {
    /// 命令作业
    Command,
    /// 脚本作业
    Script,
    /// 构建作业
    Build,
}

/// 作业状态
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "job_status", rename_all = "snake_case")]
pub enum JobStatus {
    /// 待执行
    Pending,
    /// 执行中
    Running,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已取消
    Cancelled,
    /// 部分成功（部分任务成功，部分失败）
    PartiallySucceeded,
}

/// 任务状态（单个主机执行状态）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "task_status", rename_all = "snake_case")]
pub enum TaskStatus {
    /// 待执行
    Pending,
    /// 执行中
    Running,
    /// 成功
    Succeeded,
    /// 失败
    Failed,
    /// 超时
    Timeout,
    /// 已取消
    Cancelled,
}

/// 失败原因分类
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "failure_reason", rename_all = "snake_case")]
pub enum FailureReason {
    /// 网络错误
    NetworkError,
    /// 认证失败
    AuthFailed,
    /// 连接超时
    ConnectionTimeout,
    /// 握手超时
    HandshakeTimeout,
    /// 命令超时
    CommandTimeout,
    /// 命令执行失败（非零退出码）
    CommandFailed,
    /// 未知错误
    Unknown,
}

/// 作业 - 顶层概念，代表批量执行任务
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: Uuid,
    pub job_type: JobType,
    pub name: String,
    pub description: Option<String>,
    pub status: JobStatus,

    // 目标集信息
    pub target_hosts: Json<Vec<Uuid>>,  // 固化的目标主机ID列表
    pub target_groups: Json<Vec<Uuid>>, // 目标分组ID列表

    // 执行参数
    pub command: Option<String>,     // 命令作业的命令
    pub script: Option<String>,      // 脚本作业的脚本内容
    pub script_path: Option<String>, // 脚本路径（如适用）

    // 执行配置
    pub concurrent_limit: Option<i32>, // 并发上限
    pub timeout_secs: Option<i32>,     // 超时时间（秒）
    pub retry_times: Option<i32>,      // 重试次数
    pub execute_user: Option<String>,  // 执行用户

    // 幂等性控制
    pub idempotency_key: Option<String>, // 幂等键

    // 结果统计
    pub total_tasks: i32,
    pub succeeded_tasks: i32,
    pub failed_tasks: i32,
    pub timeout_tasks: i32,
    pub cancelled_tasks: i32,

    // 审计字段
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,

    // 元数据
    pub tags: Json<Vec<String>>,
}

/// 创建命令作业请求
#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateCommandJobRequest {
    pub name: String,
    pub description: Option<String>,
    pub target_hosts: Vec<Uuid>,
    pub target_groups: Vec<Uuid>,
    pub command: String,
    pub concurrent_limit: Option<i32>,
    pub timeout_secs: Option<i32>,
    pub retry_times: Option<i32>,
    pub execute_user: Option<String>,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 创建脚本作业请求
#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateScriptJobRequest {
    pub name: String,
    pub description: Option<String>,
    pub target_hosts: Vec<Uuid>,
    pub target_groups: Vec<Uuid>,
    pub script: String,
    pub script_path: Option<String>,
    pub concurrent_limit: Option<i32>,
    pub timeout_secs: Option<i32>,
    pub retry_times: Option<i32>,
    pub execute_user: Option<String>,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 任务 - 作业的执行单元，对应单个主机
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Task {
    pub id: Uuid,
    pub job_id: Uuid,
    pub host_id: Uuid,

    // 状态信息
    pub status: TaskStatus,
    pub failure_reason: Option<FailureReason>,
    pub failure_message: Option<String>, // 详细错误信息

    // 执行信息
    pub exit_code: Option<i32>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<i64>, // 执行时长（秒）

    // 输出存档
    pub output_summary: Option<String>, // 输出摘要（用于列表展示，限制长度）
    pub output_detail: Option<String>,  // 完整输出（用于详细查询）

    // 重试信息
    pub retry_count: i32,
    pub max_retries: i32,

    // 审计字段
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 作业查询过滤器
#[derive(Debug, Deserialize, validator::Validate)]
pub struct JobListFilters {
    pub job_type: Option<JobType>,
    pub status: Option<JobStatus>,
    pub created_by: Option<Uuid>,
    pub tags: Option<Vec<String>>,
    pub search: Option<String>, // 搜索name/description
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

/// 作业列表响应（包含摘要信息）
#[derive(Debug, Serialize)]
pub struct JobSummary {
    pub id: Uuid,
    pub job_type: JobType,
    pub name: String,
    pub description: Option<String>,
    pub status: JobStatus,
    pub target_count: i32, // 目标主机数
    pub succeeded_tasks: i32,
    pub failed_tasks: i32,
    pub timeout_tasks: i32,
    pub cancelled_tasks: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<i64>, // 总执行时长
    pub tags: Vec<String>,
}

/// 任务列表响应
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    #[serde(flatten)]
    pub task: Task,
    pub host_identifier: String,
    pub host_address: String,
    pub host_display_name: Option<String>,
}

/// 取消作业请求
#[derive(Debug, Deserialize, validator::Validate)]
pub struct CancelJobRequest {
    pub reason: Option<String>,
}

/// 重试作业请求
#[derive(Debug, Deserialize, validator::Validate)]
pub struct RetryJobRequest {
    /// 只重试失败的任务
    #[serde(default)]
    pub failed_only: bool,
    /// 任务ID列表（如果指定，只重试这些任务）
    pub task_ids: Option<Vec<Uuid>>,
}

/// 作业执行统计
#[derive(Debug, Serialize)]
pub struct JobStatistics {
    pub job_id: Uuid,
    pub total_tasks: i32,
    pub succeeded_tasks: i32,
    pub failed_tasks: i32,
    pub timeout_tasks: i32,
    pub cancelled_tasks: i32,
    pub pending_tasks: i32,
    pub running_tasks: i32,
    pub success_rate: f64,              // 成功率
    pub avg_duration_secs: Option<f64>, // 平均执行时长
}
