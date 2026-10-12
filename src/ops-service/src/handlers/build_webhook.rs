//! 构建状态和日志 Webhook 处理器 (P2.1)
//!
//! 接收 Runner 回传的构建状态、步骤状态和日志更新

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use common::messages::{
    BuildArtifact, BuildLogMessage, BuildStatus, BuildStatusMessage, StepStatus, StepStatusUpdate,
};
use sqlx::{AssertSqlSafe, Row};
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    middleware::AppState,
    realtime::RealtimeEvent,
};

/// 合法状态迁移表
/// 返回 true 表示允许从当前状态迁移到新状态
fn is_valid_status_transition(current: Option<&str>, new: &str) -> bool {
    match (current, new) {
        // 新建作业可以从任何状态开始
        (None, _) => true,
        // pending -> running/completed/failed/cancelled
        (Some("pending"), "running" | "completed" | "failed" | "cancelled") => true,
        (Some("pending"), _) => false,
        // running -> completed/failed/cancelled
        (Some("running"), "completed" | "failed" | "cancelled") => true,
        (Some("running"), _) => false,
        // completed/failed/cancelled 是终态，不允许迁移
        (Some("completed"), _) => false,
        (Some("failed"), "running") => true, // 允许重试
        (Some("failed"), _) => false,
        (Some("cancelled"), "running") => true, // 允许重试
        // cancelled -> cancelled：runner 在用户取消后回传 Cancelled 终态，
        // 必须允许（幂等 no-op），否则并发额度归还逻辑被短路，runner 容量泄漏
        (Some("cancelled"), "cancelled") => true,
        (Some("cancelled"), _) => false,
        // 未知状态默认拒绝
        _ => false,
    }
}

/// 以当前状态为 CAS 条件原子地更新构建作业状态。
///
/// 此前的实现是"先读状态校验、再无条件 UPDATE"，并发回调下两条消息都会
/// 通过校验，后写者覆盖先写者（如 Succeeded 与 Failed 竞争）。加上
/// `AND status = $current` 守卫后，状态已被其他消息先行变更时本次更新
/// 放弃（返回 false），调用方应跳过后续步骤处理。
async fn update_build_job_status_cas(
    db: &sqlx::PgPool,
    job_id: Uuid,
    current_status: &str,
    new_status: &str,
) -> Result<bool> {
    // 根据状态更新时间戳
    let (started_at, completed_at) = match new_status {
        "running" => (Some(Utc::now()), None),
        "completed" | "failed" | "cancelled" => {
            let current: Option<chrono::DateTime<chrono::Utc>> =
                sqlx::query("SELECT started_at FROM build_jobs WHERE id = $1")
                    .bind(job_id)
                    .fetch_optional(db)
                    .await?
                    .and_then(|row| row.get("started_at"));
            (current, Some(Utc::now()))
        }
        _ => (None, None),
    };

    let mut query = String::from("UPDATE build_jobs SET status = CAST($1 AS job_status)");
    let mut param_idx = 2;

    if started_at.is_some() {
        query.push_str(&format!(", started_at = ${}", param_idx));
        param_idx += 1;
    }
    if completed_at.is_some() {
        query.push_str(&format!(", completed_at = ${}", param_idx));
        param_idx += 1;
    }

    // CAS 守卫：只有当前状态仍等于读取到的值时才更新。
    // 注意 status 是 job_status ENUM：sqlx 把 String 绑定为 text，
    // `status = $N` 会得到 "operator does not exist: job_status = text"
    // （PostgreSQL 不做 enum↔text 隐式比较），必须显式 ::text 后比较
    query.push_str(&format!(
        ", updated_at = NOW() WHERE id = ${} AND status::text = ${}",
        param_idx,
        param_idx + 1
    ));

    let mut query_builder = sqlx::query(AssertSqlSafe(query)).bind(new_status);
    if let Some(start) = started_at {
        query_builder = query_builder.bind(start);
    }
    if let Some(end) = completed_at {
        query_builder = query_builder.bind(end);
    }
    query_builder = query_builder.bind(job_id).bind(current_status);

    let result = query_builder.execute(db).await.map_err(|e| {
        error!(error = %e, job_id = %job_id, "Failed to update build job status");
        AppError::database("Failed to update build job")
    })?;

    Ok(result.rows_affected() > 0)
}

/// 接收构建状态更新
pub async fn build_status_webhook(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BuildStatusMessage>,
) -> Result<impl IntoResponse> {
    debug!(
        task_id = %payload.task_id,
        job_id = %payload.job_id,
        runner = %payload.runner_name,
        status = ?payload.status,
        "Received build status update"
    );

    // 检查构建作业是否存在，同时获取当前状态用于迁移校验
    let job_row = sqlx::query("SELECT id, status::text FROM build_jobs WHERE id = $1")
        .bind(payload.job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, job_id = %payload.job_id, "Failed to check build job");
            AppError::database("Failed to check build job")
        })?;

    let (current_status_opt, job_exists) = match job_row {
        Some(row) => (row.try_get::<String, _>("status").ok(), true),
        None => (None, false),
    };

    if !job_exists {
        warn!(
            job_id = %payload.job_id,
            "Build job not found for status update"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // 更新构建作业状态
    let status_str = match payload.status {
        BuildStatus::Received => "pending",
        BuildStatus::Preparing => "pending",
        BuildStatus::Running => "running",
        BuildStatus::Succeeded => "completed",
        BuildStatus::Failed => "failed",
        BuildStatus::Timeout => "failed",
        BuildStatus::Cancelled => "cancelled",
    };

    let current_status = current_status_opt.clone().unwrap_or_default();

    // 步骤状态更新在作业级迁移门控之外执行（原因同 MQ 消费路径：
    // 步骤消息携带的 Running 作业级状态会触发幂等/迁移短路，
    // 端到端实测发现步骤状态因此永远无法落库）
    if let Some(ref step_update) = payload.step_status {
        let uploaded_by = lookup_runner_id(&state, &payload.runner_name).await;
        if let Err(e) =
            update_step_status_with_uploader(&state, &payload, step_update, uploaded_by).await
        {
            error!(error = ?e, "Failed to update step status");
        } else {
            // 实时步骤状态（SSE）
            let _ = state.event_bus.publish(RealtimeEvent::BuildStepStatusChanged {
                job_id: payload.job_id,
                step_id: step_update.step_id.clone(),
                status: format!("{:?}", step_update.status).to_lowercase(),
                exit_code: step_update.exit_code,
            });
        }
    }

    // 状态迁移校验：拒绝非法迁移
    if !is_valid_status_transition(current_status_opt.as_deref(), status_str) {
        debug!(
            job_id = %payload.job_id,
            current_status = ?current_status_opt,
            new_status = %status_str,
            "Invalid status transition for job-level status, skipping job update"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // 幂等性保护：如果新旧状态相同且不是终态，跳过更新
    if current_status_opt.as_deref() == Some(status_str) {
        debug!(
            job_id = %payload.job_id,
            status = %status_str,
            "Status unchanged, skipping update"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // CAS 更新：并发回调下只有持有人读到旧状态的那条消息能落库
    let updated = update_build_job_status_cas(&state.db, payload.job_id, &current_status, status_str)
        .await?;
    if !updated {
        warn!(
            job_id = %payload.job_id,
            expected = %current_status,
            new_status = %status_str,
            "Status changed concurrently, skipping stale update"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // 发布实时事件 (SSE)
    let _ = state.event_bus.publish(RealtimeEvent::JobStatusChanged {
        job_id: payload.job_id,
        old_status: current_status_opt.unwrap_or_default(),
        new_status: status_str.to_string(),
    });

    debug!(
        job_id = %payload.job_id,
        status = %status_str,
        "Build status updated"
    );

    Ok(StatusCode::ACCEPTED)
}

/// 追加步骤日志（HTTP webhook 与 MQ 消费共用的实现）。
///
/// - 幂等：offset <= 已有 offset 的消息直接跳过；
/// - 原子：UPDATE 带 `AND log_offset < $offset` 守卫，乱序/并发写入时
///   只有持有更大 offset 的消息能落库，避免"读到旧值→校验通过→覆盖"竞态；
/// - 步骤行懒创建：upsert 同时填 build_job_id，使唯一索引真正生效。
async fn append_step_log(
    db: &sqlx::PgPool,
    job_id: Uuid,
    step_id: &str,
    content: &str,
    offset: u64,
) -> Result<bool> {
    let new_offset = offset as i64;

    let step_row = sqlx::query(
        "SELECT id, COALESCE(log_offset, 0) as log_offset FROM build_steps WHERE job_id = $1 AND step_id = $2",
    )
    .bind(job_id)
    .bind(step_id)
    .fetch_optional(db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to check build step");
        AppError::database("Failed to check build step")
    })?;

    if let Some(row) = step_row {
        let current_offset: i64 = row.try_get("log_offset").unwrap_or(0);

        // 幂等保护：offset 不大于当前值视为重复/乱序消息
        if new_offset <= current_offset && !content.is_empty() {
            debug!(
                job_id = %job_id,
                step_id = %step_id,
                current_offset,
                new_offset,
                "Duplicate or out-of-order log message, skipping"
            );
            return Ok(false);
        }

        // 原子追加：只有 log_offset 仍小于新 offset 时才允许写入
        let result = sqlx::query(
            "UPDATE build_steps
             SET output_detail = COALESCE(output_detail, '') || $1,
                 log_offset = $2,
                 updated_at = NOW()
             WHERE job_id = $3 AND step_id = $4 AND COALESCE(log_offset, 0) < $2",
        )
        .bind(content)
        .bind(new_offset)
        .bind(job_id)
        .bind(step_id)
        .execute(db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to update build step log");
            AppError::database("Failed to update build step")
        })?;

        if result.rows_affected() == 0 {
            debug!(
                job_id = %job_id,
                step_id = %step_id,
                "Log offset moved concurrently, skipping stale chunk"
            );
            return Ok(false);
        }
        return Ok(true);
    }

    // 步骤不存在：upsert 创建（并发首条消息只创建一行），
    // 同时填 build_job_id 让 (build_job_id, step_id) 唯一索引生效
    let result = sqlx::query(
        "INSERT INTO build_steps (build_job_id, job_id, step_id, step_name, status, output_detail, log_offset, created_at, updated_at)
         VALUES ($1, $1, $2, $2, 'running', $3, $4, NOW(), NOW())
         ON CONFLICT (job_id, step_id) WHERE job_id IS NOT NULL AND step_id IS NOT NULL
         DO UPDATE SET
             output_detail = COALESCE(build_steps.output_detail, '') || EXCLUDED.output_detail,
             log_offset = GREATEST(COALESCE(build_steps.log_offset, 0), EXCLUDED.log_offset),
             updated_at = NOW()",
    )
    .bind(job_id)
    .bind(step_id)
    .bind(content)
    .bind(new_offset)
    .execute(db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create build step");
        AppError::database("Failed to create build step")
    })?;

    Ok(result.rows_affected() > 0)
}

/// 接收构建日志
pub async fn build_log_webhook(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BuildLogMessage>,
) -> Result<impl IntoResponse> {
    // 检查构建作业是否存在
    let job_exists = sqlx::query("SELECT id FROM build_jobs WHERE id = $1")
        .bind(payload.job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, job_id = %payload.job_id, "Failed to check build job");
            AppError::database("Failed to check build job")
        })?;

    if job_exists.is_none() {
        warn!(job_id = %payload.job_id, "Build job not found for log");
        return Ok(StatusCode::ACCEPTED);
    }

    let appended = append_step_log(
        &state.db,
        payload.job_id,
        &payload.step_id,
        &payload.content,
        payload.offset,
    )
    .await?;

    // 实时日志 tail（SSE）
    if appended {
        let _ = state.event_bus.publish(RealtimeEvent::BuildLogAppended {
            job_id: payload.job_id,
            step_id: payload.step_id.clone(),
            content: payload.content.clone(),
            level: format!("{:?}", payload.level).to_lowercase(),
        });
    }

    debug!(
        job_id = %payload.job_id,
        step_id = %payload.step_id,
        "Build log received"
    );

    Ok(StatusCode::ACCEPTED)
}

/// 接收构建产物元数据
pub async fn build_artifact_webhook(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BuildArtifactWebhook>,
) -> Result<impl IntoResponse> {
    // 检查构建作业是否存在
    let job_exists = sqlx::query("SELECT id FROM build_jobs WHERE id = $1")
        .bind(payload.job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, job_id = %payload.job_id, "Failed to check build job");
            AppError::database("Failed to check build job")
        })?;

    if job_exists.is_none() {
        warn!(job_id = %payload.job_id, "Build job not found for artifact");
        return Ok(StatusCode::ACCEPTED);
    }

    // 检查产物是否已存在（根据版本号和类型）
    let existing =
        sqlx::query("SELECT id FROM build_artifacts WHERE version = $1 AND artifact_type = $2")
            .bind(&payload.artifact.version)
            .bind(&payload.artifact.artifact_type)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to check existing artifact");
                AppError::database("Failed to check artifact")
            })?;

    if existing.is_some() {
        // 产物已存在，拒绝覆盖上传
        warn!(
            version = %payload.artifact.version,
            artifact_type = %payload.artifact.artifact_type,
            "Artifact already exists, rejecting overwrite"
        );
        return Err(AppError::validation(
            "Artifact with this version already exists. Cannot overwrite.",
        ));
    }

    // 产物路径白名单校验（与 record_artifact 一致，防路径穿越入库）
    super::artifact::validate_artifact_path(&payload.artifact.path)?;

    // 记录产物元数据
    // check-then-insert 存在并发窗口：以 (version, artifact_type) 唯一索引兜底，
    // 冲突时返回 400 而非 500
    if let Err(e) = sqlx::query(
        "INSERT INTO build_artifacts (build_job_id, artifact_name, artifact_type, artifact_path,
                                     artifact_size, artifact_hash, version, metadata, uploaded_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(payload.job_id)
    .bind(&payload.artifact.name)
    .bind(&payload.artifact.artifact_type)
    .bind(&payload.artifact.path)
    .bind(payload.artifact.size as i64)
    .bind(&payload.artifact.sha256)
    .bind(&payload.artifact.version)
    .bind(serde_json::to_value(payload.metadata).unwrap_or_default())
    .bind(payload.uploaded_by)
    .execute(&state.db)
    .await
    {
        if let sqlx::Error::Database(db_err) = &e
            && db_err.code().as_deref() == Some("23505")
        {
            warn!(
                version = %payload.artifact.version,
                artifact_type = %payload.artifact.artifact_type,
                "Artifact already exists (unique index), rejecting overwrite"
            );
            return Err(AppError::validation(
                "Artifact with this version already exists. Cannot overwrite.",
            ));
        }
        error!(error = %e, "Failed to record artifact");
        return Err(AppError::database("Failed to record artifact"));
    }

    // 更新构建作业的产物统计
    sqlx::query(
        "UPDATE build_jobs SET has_artifacts = TRUE, artifact_count = artifact_count + 1 WHERE id = $1",
    )
    .bind(payload.job_id)
    .execute(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to update build job artifact count");
        AppError::database("Failed to update build job")
    })?;

    info!(
        job_id = %payload.job_id,
        artifact_name = %payload.artifact.name,
        version = %payload.artifact.version,
        "Artifact metadata recorded"
    );

    Ok(StatusCode::CREATED)
}

/// 根据 runner 名称查找其 UUID
async fn lookup_runner_id(state: &AppState, runner_name: &str) -> Option<Uuid> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM runners WHERE name = $1")
        .bind(runner_name)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
}

/// 更新步骤状态
async fn update_step_status(
    state: &AppState,
    status_msg: &BuildStatusMessage,
    step_update: &StepStatusUpdate,
) -> Result<()> {
    update_step_status_with_uploader(state, status_msg, step_update, None).await?;

    // 实时步骤状态（SSE）
    let _ = state.event_bus.publish(RealtimeEvent::BuildStepStatusChanged {
        job_id: status_msg.job_id,
        step_id: step_update.step_id.clone(),
        status: format!("{:?}", step_update.status).to_lowercase(),
        exit_code: step_update.exit_code,
    });
    Ok(())
}

/// 更新步骤状态（带上传者信息）
async fn update_step_status_with_uploader(
    state: &AppState,
    status_msg: &BuildStatusMessage,
    step_update: &StepStatusUpdate,
    uploaded_by: Option<Uuid>,
) -> Result<()> {
    // 转换状态
    let status_str = match step_update.status {
        StepStatus::Pending => "pending",
        StepStatus::Running => "running",
        StepStatus::Succeeded => "succeeded",
        StepStatus::Failed => "failed",
        StepStatus::Timeout => "timeout",
        StepStatus::Skipped => "skipped",
    };

    // 检查步骤是否存在
    let existing = sqlx::query("SELECT id FROM build_steps WHERE job_id = $1 AND step_id = $2")
        .bind(status_msg.job_id)
        .bind(&step_update.step_id)
        .fetch_optional(&state.db)
        .await?;

    if existing.is_some() {
        // 更新现有步骤。
        // status 是 step_status ENUM，sqlx 将 String 绑定为 text，
        // 必须显式 CAST（否则 "column status is of type step_status but
        // expression is of type text"，步骤状态永远无法更新）
        // $2/$3 必须显式标注 ::timestamptz：它们出现在 COALESCE 的参数位，
        // PostgreSQL 对无法唯一推断类型的绑定参数默认按 text 解析，
        // 会得到 "column started_at is of type timestamp with time zone but
        // expression is of type text"（端到端实测，日志懒创建路径必现）
        sqlx::query(
            "UPDATE build_steps
             SET status = $1::step_status,
                 started_at = COALESCE($2::timestamptz, started_at),
                 completed_at = COALESCE($3::timestamptz, completed_at),
                 exit_code = COALESCE($4::int, exit_code), updated_at = NOW()
             WHERE job_id = $5 AND step_id = $6",
        )
        .bind(status_str)
        .bind(step_update.started_at)
        .bind(step_update.completed_at)
        .bind(step_update.exit_code)
        .bind(status_msg.job_id)
        .bind(&step_update.step_id)
        .execute(&state.db)
        .await?;
    } else {
        // 创建新步骤记录（同时填 build_job_id，使 (build_job_id, step_id)
        // 唯一索引生效）。
        // 不用 INSERT ... ON CONFLICT DO UPDATE：部分索引冲突目标下存在
        // 跨语境参数类型推断陷阱（实测先后出现 "expression is of type
        // text" 与 "cannot cast type timestamptz to integer"）。改为纯
        // INSERT，并发撞唯一索引（23505）时回退到 UPDATE 分支语义。
        // 占位符与绑定一一对应（历史上 $1,$1,$2,$2,$3..$6 只有 6 个占位符
        // 却绑了 7 个值——step_name 顶替了 status，后续全部错位，产生
        // 一系列难以理解的类型错误）
        let insert_result = sqlx::query(
            "INSERT INTO build_steps (build_job_id, job_id, step_id, step_name, status, started_at, completed_at, exit_code, created_at, updated_at)
             VALUES ($1, $1, $2, $3, $4::step_status, $5::timestamptz, $6::timestamptz, $7::int, NOW(), NOW())",
        )
        .bind(status_msg.job_id)
        .bind(&step_update.step_id)
        .bind(&step_update.step_id) // step_name 默认使用 step_id
        .bind(status_str)
        .bind(step_update.started_at)
        .bind(step_update.completed_at)
        .bind(step_update.exit_code)
        .execute(&state.db)
        .await;

        if let Err(sqlx::Error::Database(ref db_err)) = insert_result
            && db_err.code().as_deref() == Some("23505")
        {
            // 并发下行已被其他路径（日志懒创建等）插入：按已存在语义更新
            sqlx::query(
                "UPDATE build_steps
                 SET status = $1::step_status,
                     started_at = COALESCE($2::timestamptz, started_at),
                     completed_at = COALESCE($3::timestamptz, completed_at),
                     exit_code = COALESCE($4::int, exit_code),
                     updated_at = NOW()
                 WHERE job_id = $5 AND step_id = $6",
            )
            .bind(status_str)
            .bind(step_update.started_at)
            .bind(step_update.completed_at)
            .bind(step_update.exit_code)
            .bind(status_msg.job_id)
            .bind(&step_update.step_id)
            .execute(&state.db)
            .await?;
        } else if let Err(e) = insert_result {
            return Err(e.into());
        }
    }

    // 如果有产物，记录产物信息
    // uploaded_by 是 NOT NULL 的 users 外键：runner 无法解析时不能伪造
    // 随机 UUID（产生悬挂外键/垃圾数据），此时跳过记录并告警
    if let Some(artifact) = &step_update.artifact {
        // 产物路径白名单校验（与 record_artifact 一致，防路径穿越入库）
        super::artifact::validate_artifact_path(&artifact.path)?;
        let Some(uploader) = uploaded_by else {
            warn!(
                job_id = %status_msg.job_id,
                step_id = %step_update.step_id,
                "Unknown runner for step artifact upload, skipping artifact record"
            );
            return Ok(());
        };
        sqlx::query(
            "INSERT INTO build_artifacts (build_job_id, artifact_name, artifact_type, artifact_path,
                                         artifact_size, artifact_hash, version, metadata, uploaded_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (version, artifact_type) DO NOTHING",
        )
        .bind(status_msg.job_id)
        .bind(&artifact.name)
        .bind(&artifact.artifact_type)
        .bind(&artifact.path)
        .bind(artifact.size as i64)
        .bind(&artifact.sha256)
        .bind(&artifact.version)
        .bind(serde_json::json!({
            "step_id": step_update.step_id,
            "produced_at": Utc::now(),
        }))
        .bind(uploader)
        .execute(&state.db)
        .await?;
    }

    Ok(())
}

/// 构建产物 Webhook 请求
#[derive(serde::Deserialize)]
pub struct BuildArtifactWebhook {
    /// 构建作业 ID
    pub job_id: Uuid,
    /// 任务 ID
    pub task_id: Uuid,
    /// 产物信息
    pub artifact: BuildArtifact,
    /// 产物元数据
    #[serde(flatten)]
    pub metadata: serde_json::Value,
    /// 上传者（触发者）
    pub uploaded_by: Uuid,
}

// ==================== RabbitMQ 消费服务 ====================

/// RabbitMQ 构建消息消费服务
/// 用于消费 Runner 回传的状态和日志消息（替代 HTTP webhook）
#[derive(Clone)]
pub struct BuildMessageConsumer {
    state: Arc<AppState>,
}

impl BuildMessageConsumer {
    /// 创建新的消费服务
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// 处理状态消息（从 RabbitMQ）
    pub async fn handle_status_message(&self, data: Vec<u8>) -> anyhow::Result<()> {
        let payload: BuildStatusMessage = serde_json::from_slice(&data)
            .map_err(|e| anyhow::anyhow!("Failed to parse status message: {}", e))?;

        debug!(
            task_id = %payload.task_id,
            job_id = %payload.job_id,
            runner = %payload.runner_name,
            status = ?payload.status,
            "Processing build status from RabbitMQ"
        );

        // 获取当前作业状态（用于判断是否需要递减 current_jobs）
        let current_status: Option<String> =
            sqlx::query_scalar::<_, String>("SELECT status::text FROM build_jobs WHERE id = $1")
                .bind(payload.job_id)
                .fetch_optional(&self.state.db)
                .await?;

        if current_status.is_none() {
            warn!(
                job_id = %payload.job_id,
                "Build job not found for status update"
            );
            return Ok(());
        }

        // 更新构建作业状态
        let status_str = match payload.status {
            BuildStatus::Received => "pending",
            BuildStatus::Preparing => "pending",
            BuildStatus::Running => "running",
            BuildStatus::Succeeded => "completed",
            BuildStatus::Failed => "failed",
            BuildStatus::Timeout => "failed",
            BuildStatus::Cancelled => "cancelled",
        };

        // 步骤状态更新必须在作业级迁移门控之外执行：
        // runner 的步骤消息都携带 Running 作业级状态，第二次起
        // running->running 会被迁移校验提前 return，步骤状态永远无法落库
        // （端到端实测定位）
        if let Some(ref step_update) = payload.step_status
            && let Err(e) = update_step_status(&self.state, &payload, step_update).await
        {
            error!(error = ?e, "Failed to update step status");
        }

        // 状态迁移校验 + CAS 原子更新（与 HTTP webhook 路径共用实现）
        if !is_valid_status_transition(current_status.as_deref(), status_str) {
            debug!(
                job_id = %payload.job_id,
                current_status = %current_status.clone().unwrap_or_default(),
                new_status = %status_str,
                "Invalid status transition for job-level status, skipping job update"
            );
            return Ok(());
        }

        let updated = update_build_job_status_cas(
            &self.state.db,
            payload.job_id,
            current_status.as_deref().unwrap_or_default(),
            status_str,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update build job: {}", e))?;
        if !updated {
            debug!(
                job_id = %payload.job_id,
                status = %status_str,
                "Status changed concurrently, skipping stale update"
            );
        }

        // 递减 Runner current_jobs（仅在任务从运行状态转为最终状态时）
        // 避免重复递减：只有当前状态是 running/pending 且新状态是最终状态时才递减
        let is_terminal_status = matches!(
            payload.status,
            BuildStatus::Succeeded
                | BuildStatus::Failed
                | BuildStatus::Timeout
                | BuildStatus::Cancelled
        );
        // was_running 含 cancelled：用户在 runner 执行中取消时 DB 已是
        // cancelled，runner 仍会回传终态，此时也要归还并发额度
        let was_running = matches!(
            current_status.as_deref(),
            Some("running") | Some("pending") | Some("cancelled")
        );
        let actually_transitioned = updated;

        // 终态通知（fire-and-forget：通知失败不影响主流程）
        if is_terminal_status && actually_transitioned {
            if let Some(notify) = &self.state.notify_service {
                let project = sqlx::query_scalar::<_, Option<String>>(
                    "SELECT j.name FROM jobs j JOIN build_jobs bj ON bj.job_id = j.id WHERE bj.id = $1",
                )
                .bind(payload.job_id)
                .fetch_optional(&self.state.db)
                .await
                .ok()
                .flatten()
                .flatten()
                .unwrap_or_else(|| payload.job_id.to_string());
                let notify = notify.clone();
                let status = status_str.to_string();
                let job = payload.job_id;
                tokio::spawn(async move {
                    notify
                        .notify_terminal(
                            "构建终态通知",
                            &format!("项目: {project}\n状态: {status}\n作业: {job}"),
                        )
                        .await;
                });
            }
        }

        if is_terminal_status && was_running && actually_transitioned {
            match Self::decrement_runner_jobs(&self.state, &payload.runner_name).await {
                Err(e) => {
                    error!(error = ?e, runner = %payload.runner_name, "Failed to decrement runner current_jobs");
                }
                _ => {
                    debug!(runner = %payload.runner_name, job_id = %payload.job_id, "Decremented runner current_jobs");
                }
            }
        }

        debug!(job_id = %payload.job_id, status = %status_str, "Build status updated from RabbitMQ");
        Ok(())
    }

    /// 根据 runner_name 递减 current_jobs
    async fn decrement_runner_jobs(state: &AppState, runner_name: &str) -> anyhow::Result<()> {
        let row = sqlx::query("SELECT id FROM runners WHERE name = $1")
            .bind(runner_name)
            .fetch_optional(&state.db)
            .await?;

        if let Some(row) = row {
            let runner_id: uuid::Uuid = row.get("id");
            state
                .runner_scheduler
                .decrement_current_jobs(runner_id)
                .await?;
        } else {
            warn!(runner = %runner_name, "Runner not found for decrement");
        }
        Ok(())
    }

    /// 处理日志消息（从 RabbitMQ）
    ///
    /// 与 HTTP webhook 路径共用 append_step_log：offset 幂等 + 原子追加，
    /// 此前 MQ 路径完全没有幂等检查，重复/乱序投递会直接重复追加。
    pub async fn handle_log_message(&self, data: Vec<u8>) -> anyhow::Result<()> {
        let payload: BuildLogMessage = serde_json::from_slice(&data)
            .map_err(|e| anyhow::anyhow!("Failed to parse log message: {}", e))?;

        // 检查构建作业是否存在
        let job_exists = sqlx::query("SELECT id FROM build_jobs WHERE id = $1")
            .bind(payload.job_id)
            .fetch_optional(&self.state.db)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check build job: {}", e))?;

        if job_exists.is_none() {
            warn!(job_id = %payload.job_id, "Build job not found for log");
            return Ok(());
        }

        let appended = append_step_log(
            &self.state.db,
            payload.job_id,
            &payload.step_id,
            &payload.content,
            payload.offset,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to append step log: {}", e))?;

        // 实时日志 tail（SSE）
        if appended {
            let _ = self.state.event_bus.publish(RealtimeEvent::BuildLogAppended {
                job_id: payload.job_id,
                step_id: payload.step_id.clone(),
                content: payload.content.clone(),
                level: format!("{:?}", payload.level).to_lowercase(),
            });
        }

        debug!(
            job_id = %payload.job_id,
            step_id = %payload.step_id,
            "Build log processed from RabbitMQ"
        );
        Ok(())
    }
}
