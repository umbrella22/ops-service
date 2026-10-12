//! 构建作业 API 处理器 (P2.1)
//!
//! 提供构建作业的创建、查询、取消、重试等功能

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

use common::messages::*;

use crate::{
    auth::AuthContext,
    error::{AppError, Result},
    middleware::AppState,
};

use crate::services::audit_service::AuditLogParams;
use sqlx::{AssertSqlSafe, Row};

/// 创建构建作业请求
#[derive(Debug, Deserialize)]
pub struct CreateBuildJobRequest {
    /// 项目名称
    pub project_name: String,

    /// 仓库 URL
    pub repository_url: String,

    /// 分支
    pub branch: String,

    /// Commit SHA (可选，默认使用最新)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,

    /// 构建类型
    pub build_type: String,

    /// 环境变量
    #[serde(default)]
    pub env_vars: std::collections::HashMap<String, String>,

    /// 构建参数
    #[serde(default)]
    pub parameters: std::collections::HashMap<String, serde_json::Value>,

    /// 构建步骤（与 pipeline_file 二选一；为空时 Runner 从仓库读管道文件）
    pub steps: Vec<BuildStepRequest>,

    /// Pipeline-as-code：仓库内管道文件路径（缺省 .ops.yml）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_file: Option<String>,

    /// 发布目标（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_target: Option<PublishTargetRequest>,

    /// 超时时间（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<i32>,

    /// 标签
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// 构建步骤请求
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildStepRequest {
    /// 步骤 ID
    pub id: String,

    /// 步骤名称
    pub name: String,

    /// 步骤类型
    pub step_type: String,

    /// 命令
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// 脚本内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,

    /// 工作目录
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// 超时（秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// 失败后是否继续
    #[serde(default)]
    pub continue_on_failure: bool,

    /// 是否产生产物
    #[serde(default)]
    pub produces_artifact: bool,

    /// Docker 镜像
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_image: Option<String>,
}

/// 发布目标请求
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PublishTargetRequest {
    /// 目标类型
    pub target_type: String,

    /// 目标地址
    pub url: String,

    /// 认证信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthInfoRequest>,
}

/// 认证信息请求
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthInfoRequest {
    /// 认证类型
    pub auth_type: String,

    /// 用户名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// 密码/Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// API Key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// 构建作业响应
#[derive(Debug, Serialize)]
pub struct BuildJobResponse {
    /// 构建作业 ID
    pub id: Uuid,

    /// 项目名称
    pub project_name: String,

    /// 仓库 URL
    pub repository_url: String,

    /// 分支
    pub branch: String,

    /// Commit SHA
    pub commit: String,

    /// 构建类型
    pub build_type: String,

    /// 状态
    pub status: String,

    /// 步骤状态
    pub steps: Vec<BuildStepResponse>,

    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// 开始时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 完成时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 创建者
    pub created_by: Uuid,

    /// 标签
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// 构建步骤响应
#[derive(Debug, Serialize)]
pub struct BuildStepResponse {
    /// 步骤 ID
    pub id: String,

    /// 步骤名称
    pub name: String,

    /// 状态
    pub status: String,

    /// 开始时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 完成时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 退出码
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 构建作业列表响应
#[derive(Debug, Serialize)]
pub struct BuildJobListResponse {
    /// 构建作业列表
    pub jobs: Vec<BuildJobSummary>,

    /// 总数
    pub total: i64,

    /// 分页信息
    pub page: i64,
    pub per_page: i64,
}

/// 构建作业摘要
#[derive(Debug, Serialize)]
pub struct BuildJobSummary {
    /// 构建作业 ID
    pub id: Uuid,

    /// 项目名称
    pub project_name: String,

    /// 仓库 URL
    pub repository_url: String,

    /// 分支
    pub branch: String,

    /// Commit SHA
    pub commit: String,

    /// 构建类型
    pub build_type: String,

    /// 状态
    pub status: String,

    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// 开始时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 完成时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,

    /// 创建者
    pub created_by: Uuid,
}

// ==================== 构建作业 API ====================

/// 创建构建作业
pub async fn create_build_job(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Json(request): Json<CreateBuildJobRequest>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "execute", None, None)
        .await?;
    let response = create_build_job_core(&state, request, auth.user_id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// 创建构建作业的核心实现（API 与 Git webhook 共用）。
/// 权限检查由调用方负责；返回 BuildJobResponse（含 job id）。
pub(crate) async fn create_build_job_core(
    state: &Arc<AppState>,
    request: CreateBuildJobRequest,
    user_id: Uuid,
) -> Result<BuildJobResponse> {
    // 验证请求
    validate_build_request(&request)?;

    let job_id = Uuid::new_v4();
    let commit = request
        .commit
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "".to_string());
    let now = Utc::now();

    // 将步骤序列化用于 build_parameters
    let build_params = serde_json::to_value(&request.steps)
        .map_err(|e| AppError::internal_error(&format!("Failed to serialize steps: {}", e)))?;

    // 插入构建作业记录
    // 注意: build_jobs 表需要先有一个 parent jobs 记录；
    // 两条 INSERT 必须在同一事务中，否则第二步失败会留下孤儿父作业
    let parent_job_id = Uuid::new_v4();

    let mut tx = state.db.begin().await.map_err(|e| {
        error!(error = %e, "Failed to begin transaction");
        AppError::database("Failed to begin transaction")
    })?;

    // 先创建 parent jobs 记录
    sqlx::query(
        "INSERT INTO jobs (id, job_type, name, description, status, target_hosts, target_groups, \
         total_tasks, created_by, tags)
         VALUES ($1, 'build'::job_type, $2, $3, 'pending'::job_status, '{}'::jsonb, '{}'::jsonb, 1, $4, $5)",
    )
    .bind(parent_job_id)
    .bind(&request.project_name)
    .bind(format!("Build: {}", request.project_name))
    .bind(user_id)
    .bind(serde_json::to_value(&request.tags).unwrap_or(serde_json::json!([])))
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create parent job record");
        AppError::database("Failed to create parent job")
    })?;

    // 插入 build_jobs 记录
    sqlx::query(
        "INSERT INTO build_jobs (id, job_id, repository, branch, commit_hash, build_type, \
         build_parameters, runner_capability, status, triggered_by, tags)
         VALUES ($1, $2, $3, $4, $5, CAST($6 AS build_type), $7, CAST($8 AS runner_capability), CAST($9 AS job_status), $10, $11)",
    )
    .bind(job_id)
    .bind(parent_job_id)
    .bind(&request.repository_url)
    .bind(&request.branch)
    .bind(&commit)
    .bind(&request.build_type)
    .bind(serde_json::json!({
        "steps": build_params,
        "parameters": request.parameters,
        "env_vars": request.env_vars,
        "pipeline_file": request.pipeline_file,
    }))
    .bind(runner_capability_for(&request.build_type)) // 两个 ENUM 取值集不同，不能原样镜像
    .bind("pending")
    .bind(user_id)
    .bind(serde_json::to_value(&request.tags).unwrap_or(serde_json::json!([])))
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create build job");
        AppError::database("Failed to create build job")
    })?;

    tx.commit().await.map_err(|e| {
        error!(error = %e, "Failed to commit build job creation");
        AppError::database("Failed to commit build job")
    })?;

    // 记录审计日志
    let _ = state
        .audit_service
        .log_action(AuditLogParams {
            subject_id: user_id,
            subject_type: "user",
            subject_name: None,
            action: crate::services::audit_service::AuditAction::BuildCreate.as_str(),
            resource_type: "build",
            resource_id: Some(job_id),
            resource_name: Some(&request.project_name),
            changes: Some(serde_json::json!({
                "branch": request.branch,
                "build_type": request.build_type,
            })),
            changes_summary: None,
            source_ip: None,
            user_agent: None,
            trace_id: None,
            result: "success",
            error_message: None,
        })
        .await;

    info!(
        job_id = %job_id,
        project = %request.project_name,
        branch = %request.branch,
        "Build job created"
    );

    // 派发构建任务到 RabbitMQ
    let task_id = Uuid::new_v4();

    // 构建任务消息
    let build_task = BuildTaskMessage {
        task_id,
        job_id,
        project: ProjectInfo {
            name: request.project_name.clone(),
            repository_url: request.repository_url.clone(),
            branch: request.branch.clone(),
            commit: commit.clone(),
            triggered_by: user_id,
        },
        build: BuildParameters {
            build_type: request.build_type.clone(),
            env_vars: request.env_vars.clone(),
            parameters: request.parameters.clone(),
            timeout_secs: request.timeout_secs.map(|t| t.max(0) as u64),
        },
        steps: request
            .steps
            .iter()
            .map(|s| BuildStep {
                id: s.id.clone(),
                name: s.name.clone(),
                step_type: parse_step_type(&s.step_type),
                command: s.command.clone(),
                script: s.script.clone(),
                working_dir: s.working_dir.clone(),
                timeout_secs: s.timeout_secs,
                continue_on_failure: s.continue_on_failure,
                produces_artifact: s.produces_artifact,
                docker_image: s.docker_image.clone(),
            })
            .collect(),
        publish_target: request.publish_target.as_ref().map(|pt| PublishTarget {
            target_type: pt.target_type.clone(),
            url: pt.url.clone(),
            auth: pt.auth.as_ref().map(|a| AuthInfo {
                auth_type: a.auth_type.clone(),
                username: a.username.clone(),
                token: a.token.clone(),
                api_key: a.api_key.clone(),
            }),
        }),
        pipeline_file: request.pipeline_file.clone(),
    };

    // 发布到 RabbitMQ
    match state.rabbitmq_publisher.get().await {
        Ok(_) => {
            match dispatch_build_task(&state, &build_task, &request.build_type).await {
                Err(e) => {
                    error!(error = %e, "Failed to dispatch build task to RabbitMQ");
                    // 不阻塞响应，但记录错误
                }
                _ => {
                    info!(
                        job_id = %job_id,
                        task_id = %task_id,
                        "Build task dispatched to RabbitMQ"
                    );
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "RabbitMQ publisher not available, build will not be dispatched");
        }
    }

    Ok(BuildJobResponse {
        id: job_id,
        project_name: request.project_name,
        repository_url: request.repository_url,
        branch: request.branch,
        commit,
        build_type: request.build_type,
        status: "pending".to_string(),
        steps: request
            .steps
            .iter()
            .map(|s| BuildStepResponse {
                id: s.id.clone(),
                name: s.name.clone(),
                status: "pending".to_string(),
                started_at: None,
                completed_at: None,
                exit_code: None,
                error: None,
            })
            .collect(),
        created_at: now,
        started_at: None,
        completed_at: None,
        created_by: user_id,
        tags: request.tags,
    })
}

/// 构建作业列表分页参数
#[derive(Debug, Deserialize)]
pub struct BuildJobListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub build_type: Option<String>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub per_page: Option<i64>,
}

/// 查询构建作业列表
pub async fn list_build_jobs(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    axum::extract::Query(query): axum::extract::Query<BuildJobListQuery>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "read", None, None)
        .await?;

    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);

    // 真分页：page/per_page 钳制 + 独立 COUNT（此前 LIMIT 100 写死、total 恒等于页内条数）
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let mut where_clauses: Vec<String> = Vec::new();
    let mut bind_values: Vec<String> = Vec::new();
    if !is_admin {
        // triggered_by 走参数化绑定
        where_clauses.push(format!("bj.triggered_by = ${}", bind_values.len() + 1));
        bind_values.push(auth.user_id.to_string());
    }
    if let Some(status) = &query.status {
        where_clauses.push(format!("bj.status::text = ${}", bind_values.len() + 1));
        bind_values.push(status.clone());
    }
    if let Some(build_type) = &query.build_type {
        where_clauses.push(format!("bj.build_type::text = ${}", bind_values.len() + 1));
        bind_values.push(build_type.clone());
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let count_sql2 = format!(
        "SELECT COUNT(*) FROM build_jobs bj JOIN jobs j ON j.id = bj.job_id {}",
        where_sql
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql2));
    {
        let mut idx = 0usize;
        if !is_admin {
            count_q = count_q.bind(auth.user_id);
            idx += 1;
        }
        if let Some(status) = &query.status {
            let _ = idx;
            count_q = count_q.bind(status.clone());
        }
        if let Some(build_type) = &query.build_type {
            count_q = count_q.bind(build_type.clone());
        }
    }
    let total = count_q.fetch_one(&state.db).await.map_err(|e| {
        error!(error = %e, "Failed to count build jobs");
        AppError::database("Failed to list build jobs")
    })?;

    let base_select = "SELECT bj.id, j.name AS project_name, bj.repository AS repository_url, \
            bj.branch, bj.commit_hash AS commit, bj.build_type::text, bj.status::text, \
            bj.created_at, bj.started_at, bj.completed_at, bj.triggered_by AS created_by
     FROM build_jobs bj
     JOIN jobs j ON j.id = bj.job_id";
    let query_sql = format!(
        "{} {} ORDER BY bj.created_at DESC LIMIT ${} OFFSET ${}",
        base_select,
        where_sql,
        bind_values.len() + 1,
        bind_values.len() + 2
    );
    let mut q = sqlx::query_as::<_, BuildJobRow>(AssertSqlSafe(query_sql));
    if !is_admin {
        q = q.bind(auth.user_id);
    }
    if let Some(status) = &query.status {
        q = q.bind(status.clone());
    }
    if let Some(build_type) = &query.build_type {
        q = q.bind(build_type.clone());
    }
    q = q.bind(per_page).bind(offset);

    #[derive(sqlx::FromRow)]
    struct BuildJobRow {
        id: Uuid,
        project_name: String,
        repository_url: String,
        branch: String,
        commit: String,
        build_type: String,
        status: String,
        created_at: chrono::DateTime<chrono::Utc>,
        started_at: Option<chrono::DateTime<chrono::Utc>>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        created_by: Uuid,
    }

    let jobs: Vec<BuildJobRow> = q
        .fetch_all(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to list build jobs");
            AppError::database("Failed to list build jobs")
        })?;

    Ok(Json(BuildJobListResponse {
        jobs: jobs
            .into_iter()
            .map(|row| BuildJobSummary {
                id: row.id,
                project_name: row.project_name,
                repository_url: row.repository_url,
                branch: row.branch,
                commit: row.commit,
                build_type: row.build_type,
                status: row.status,
                created_at: row.created_at,
                started_at: row.started_at,
                completed_at: row.completed_at,
                created_by: row.created_by,
            })
            .collect(),
        total,
        page: 1,
        per_page: 100,
    }))
}

/// 获取构建作业详情
pub async fn get_build_job(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "read", None, None)
        .await?;

    // 查询构建作业
    #[derive(sqlx::FromRow)]
    struct BuildJobRow {
        id: Uuid,
        project_name: String,
        repository_url: String,
        branch: String,
        commit: String,
        build_type: String,
        status: String,
        build_parameters: serde_json::Value,
        created_at: chrono::DateTime<chrono::Utc>,
        started_at: Option<chrono::DateTime<chrono::Utc>>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        created_by: Uuid,
        tags: Option<serde_json::Value>,
    }

    let job: BuildJobRow = sqlx::query_as(
        "SELECT bj.id, j.name AS project_name, bj.repository AS repository_url,
                bj.branch, bj.commit_hash AS commit, bj.build_type::text, bj.status::text,
                bj.build_parameters, bj.created_at, bj.started_at, bj.completed_at,
                bj.triggered_by AS created_by, bj.tags
         FROM build_jobs bj
         JOIN jobs j ON j.id = bj.job_id
         WHERE bj.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to get build job");
        AppError::database("Failed to get build job")
    })?
    .ok_or_else(|| AppError::not_found("Build job not found"))?;

    // 检查权限
    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_admin && job.created_by != auth.user_id {
        return Err(AppError::not_found("Build job not found")); // 反枚举：返回 404 而非 403
    }

    // 解析步骤
    let steps_request: Vec<BuildStepRequest> = job
        .build_parameters
        .get("steps")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    // Pipeline-as-code 模式：API 未传 steps，实际步骤在 Runner 解析管道文件后
    // 经步骤状态消息落库——详情页从 build_steps 表回显
    let pipeline_mode = steps_request.is_empty();

    // 查询步骤状态
    #[derive(sqlx::FromRow)]
    struct StepStatusRow {
        step_id: String,
        status: String,
        started_at: Option<chrono::DateTime<chrono::Utc>>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        exit_code: Option<i32>,
        error: Option<String>,
    }

    let step_statuses: Vec<StepStatusRow> = sqlx::query_as(
        "SELECT step_id, status::text, started_at, completed_at, exit_code, error
         FROM build_steps
         WHERE job_id = $1
         ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    let step_status_map: std::collections::HashMap<String, StepStatusRow> = step_statuses
        .into_iter()
        .map(|s| (s.step_id.clone(), s))
        .collect();

    let steps: Vec<BuildStepResponse> = if pipeline_mode {
        // Pipeline-as-code：实际步骤以 Runner 解析管道文件后上报的
        // build_steps 行为准（API 侧的 steps 列表为空）
        let mut rows: Vec<BuildStepResponse> = step_status_map
            .values()
            .map(|st| BuildStepResponse {
                id: st.step_id.clone(),
                name: st.step_id.clone(),
                status: st.status.clone(),
                started_at: st.started_at,
                completed_at: st.completed_at,
                exit_code: st.exit_code,
                error: st.error.clone(),
            })
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    } else {
        steps_request
            .iter()
            .map(|s| {
                let status = step_status_map.get(&s.id);
                BuildStepResponse {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    status: status
                        .map(|st| st.status.clone())
                        .unwrap_or_else(|| "pending".to_string()),
                    started_at: status.and_then(|st| st.started_at),
                    completed_at: status.and_then(|st| st.completed_at),
                    exit_code: status.and_then(|st| st.exit_code),
                    error: status.and_then(|st| st.error.clone()),
                }
            })
            .collect()
    };

    // 解析标签
    let tags: Option<Vec<String>> = job.tags.and_then(|v| serde_json::from_value(v).ok());

    Ok(Json(BuildJobResponse {
        id: job.id,
        project_name: job.project_name,
        repository_url: job.repository_url,
        branch: job.branch,
        commit: job.commit,
        build_type: job.build_type,
        status: job.status,
        steps,
        created_at: job.created_at,
        started_at: job.started_at,
        completed_at: job.completed_at,
        created_by: job.created_by,
        tags,
    }))
}

/// 订阅构建作业实时事件流（SSE：作业/步骤状态 + 实时日志 tail）
pub async fn subscribe_build_events(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "output_detail", None, None)
        .await?;

    // 非管理员只能订阅自己触发的构建（反枚举 404）
    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_admin {
        let owner: Option<Uuid> = sqlx::query_scalar("SELECT triggered_by FROM build_jobs WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to check build owner");
                AppError::database("Failed to check build job")
            })?;
        match owner {
            Some(created_by) if created_by == auth.user_id => {}
            _ => return Err(AppError::not_found("Build job not found")),
        }
    }

    let stream = state
        .event_bus
        .subscribe_to_job(id)
        .to_sse_stream()
        .await?;

    let body = axum::body::Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .map_err(|e| AppError::internal_error(&format!("Failed to create SSE response: {}", e)))
}

/// 获取构建作业步骤状态
pub async fn get_build_steps(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "output_detail", None, None)
        .await?;

    // 首先检查作业是否存在以及权限
    // build_jobs 的属主列是 triggered_by（created_by 平行列从不填充，
    // 读它必然 NULL 解码 panic）
    let job = sqlx::query("SELECT triggered_by AS created_by FROM build_jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get build job");
            AppError::database("Failed to get build job")
        })?
        .ok_or_else(|| AppError::not_found("Build job not found"))?;

    let created_by: Uuid = job.get::<Uuid, _>("created_by");

    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_admin && created_by != auth.user_id {
        return Err(AppError::not_found("Build job not found")); // 反枚举
    }

    #[derive(sqlx::FromRow, Serialize)]
    struct StepRow {
        step_id: String,
        step_name: String,
        status: String,
        started_at: Option<chrono::DateTime<chrono::Utc>>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
        exit_code: Option<i32>,
        error: Option<String>,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    let steps: Vec<StepRow> = sqlx::query_as(
        "SELECT step_id, step_name, status::text, started_at, completed_at, exit_code, error, created_at
         FROM build_steps
         WHERE job_id = $1
         ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to get build steps");
        AppError::database("Failed to get build steps")
    })?;

    Ok(Json(serde_json::json!({
        "job_id": id,
        "steps": steps,
    })))
}

/// 取消构建作业
pub async fn cancel_build_job(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "execute", None, None)
        .await?;

    // 查询作业
    // 端到端实测发现：原 SELECT 不含 created_by，下方 .get("created_by")
    // 对缺失列直接 panic（任何取消请求必炸）。build_jobs 的属主列是
    // triggered_by，别名读出。
    let job = sqlx::query(
        "SELECT triggered_by AS created_by, status::text FROM build_jobs WHERE id = $1",
    )
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to get build job");
            AppError::database("Failed to get build job")
        })?
        .ok_or_else(|| AppError::not_found("Build job not found"))?;

    let created_by: Uuid = job.get::<Uuid, _>("created_by");
    let status: String = job.get::<String, _>("status");

    // 检查权限（反枚举：返回 404 而非 403）
    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_admin && created_by != auth.user_id {
        return Err(AppError::not_found("Build job not found"));
    }

    // 检查状态
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(AppError::validation(&format!(
            "Cannot cancel build job in status: {}",
            status
        )));
    }

    // CAS 更新：只有仍处于 pending/running 时才能取消
    // （旧实现 check-then-act，与 runner 终态回执竞态时会把 completed 覆盖成 cancelled）
    let updated = sqlx::query(
        "UPDATE build_jobs SET status = 'cancelled', completed_at = NOW(), updated_at = NOW() \
         WHERE id = $1 AND status IN ('pending', 'running')",
    )
    .bind(id)
    .execute(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to cancel build job");
        AppError::database("Failed to cancel build job")
    })?;

    if updated.rows_affected() == 0 {
        return Err(AppError::validation("Build job is no longer running"));
    }

    // 推送实时事件（SSE 订阅者立即感知取消）
    let _ = state.event_bus.publish(crate::realtime::RealtimeEvent::JobStatusChanged {
        job_id: id,
        old_status: status.clone(),
        new_status: "cancelled".to_string(),
    });

    // 下发取消消息到 runner（广播 build.cancel.<job_id>）。
    // 派发失败不阻塞取消响应：DB 已是终态，runner 若继续执行，
    // 其终态回执会被状态机拒绝（cancelled -> * 非法迁移），仅在日志可见。
    let cancel_msg = BuildCancelMessage {
        job_id: id,
        task_id: None,
        requested_by: Some(auth.user_id),
        reason: Some("cancelled by user".to_string()),
        timestamp: Utc::now(),
    };
    match serde_json::to_vec(&cancel_msg) {
        Ok(payload) => match state.rabbitmq_publisher.get().await {
            Ok(publisher) => {
                if let Err(e) = publisher.publish_build_cancel(id, &payload).await {
                    error!(error = %e, job_id = %id, "Failed to publish build cancel message");
                }
            }
            Err(e) => {
                warn!(error = %e, job_id = %id, "RabbitMQ unavailable, cancel message not dispatched");
            }
        },
        Err(e) => {
            error!(error = %e, job_id = %id, "Failed to serialize cancel message");
        }
    }

    // 记录审计日志
    let _ = state
        .audit_service
        .log_action(AuditLogParams {
            subject_id: auth.user_id,
            subject_type: "user",
            subject_name: None,
            action: crate::services::audit_service::AuditAction::BuildCancel.as_str(),
            resource_type: "build",
            resource_id: Some(id),
            resource_name: None,
            changes: None,
            changes_summary: None,
            source_ip: None,
            user_agent: None,
            trace_id: None,
            result: "success",
            error_message: None,
        })
        .await;

    info!(job_id = %id, "Build job cancelled");

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "id": id,
            "status": "cancelled"
        })),
    ))
}

/// 重试构建作业
pub async fn retry_build_job(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "execute", None, None)
        .await?;

    // 查询原作业。
    // 注意：schema 中 build_jobs 存在两套平行列（创建路径写入
    // repository/commit_hash/build_parameters/triggered_by；000008 又补了
    // project_name/repository_url/... 但从未被创建路径填充）。此前的重试
    // 读取的是空置的那套列，对 API 创建的作业必然解码 NULL 失败。这里统一
    // 使用与 create_build_job 相同的列集。
    let original: OriginalJob = sqlx::query_as(
        "SELECT repository, branch, COALESCE(commit_hash, '') AS commit_hash, \
         build_type::text AS build_type, COALESCE(build_parameters, '{}'::jsonb) AS build_parameters, \
         runner_capability::text AS runner_capability, triggered_by, status::text AS status, tags, job_id
         FROM build_jobs
         WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to get build job");
        AppError::database("Failed to get build job")
    })?
    .ok_or_else(|| AppError::not_found("Build job not found"))?;

    // 检查权限（反枚举：返回 404 而非 403）
    let is_admin = state
        .permission_service
        .is_admin(auth.user_id)
        .await
        .unwrap_or(false);
    if !is_admin && original.triggered_by != auth.user_id {
        return Err(AppError::not_found("Build job not found"));
    }

    // 终态校验：只有已结束的作业可以重试
    if !matches!(original.status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(AppError::validation(&format!(
            "Cannot retry build job in status: {}",
            original.status
        )));
    }

    // 提取项目名：parent jobs 行的 name（列表页与详情页均以此为 project_name）
    let project_name: String = match original.job_id {
        Some(parent_id) => sqlx::query_scalar("SELECT name FROM jobs WHERE id = $1")
            .bind(parent_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to get parent job name");
                AppError::database("Failed to get build job")
            })?
            .unwrap_or_else(|| "retry".to_string()),
        None => "retry".to_string(),
    };

    // 创建新的构建作业（与创建路径相同的事务与列集）
    let new_job_id = Uuid::new_v4();
    let new_parent_job_id = Uuid::new_v4();

    let mut tx = state.db.begin().await.map_err(|e| {
        error!(error = %e, "Failed to begin transaction");
        AppError::database("Failed to begin transaction")
    })?;

    sqlx::query(
        "INSERT INTO jobs (id, job_type, name, description, status, target_hosts, target_groups, \
         total_tasks, created_by, tags)
         VALUES ($1, 'build'::job_type, $2, $3, 'pending'::job_status, '{}'::jsonb, '{}'::jsonb, 1, $4, $5)",
    )
    .bind(new_parent_job_id)
    .bind(&project_name)
    .bind(format!("Build (retry): {}", project_name))
    .bind(auth.user_id)
    .bind(&original.tags)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create retry parent job");
        AppError::database("Failed to create retry parent job")
    })?;

    sqlx::query(
        "INSERT INTO build_jobs (id, job_id, repository, branch, commit_hash, build_type, \
         build_parameters, runner_capability, status, triggered_by, tags, retry_of)
         VALUES ($1, $2, $3, $4, $5, CAST($6 AS build_type), $7, CAST($8 AS runner_capability), \
         'pending'::job_status, $9, $10, $11)",
    )
    .bind(new_job_id)
    .bind(new_parent_job_id)
    .bind(&original.repository)
    .bind(&original.branch)
    .bind(&original.commit_hash)
    .bind(&original.build_type)
    .bind(&original.build_parameters)
    .bind(&original.runner_capability)
    .bind(auth.user_id)
    .bind(&original.tags)
    .bind(id)  // retry_of 指向原作业
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create retry build job");
        AppError::database("Failed to create retry build job")
    })?;

    tx.commit().await.map_err(|e| {
        error!(error = %e, "Failed to commit retry build job");
        AppError::database("Failed to commit retry build job")
    })?;

    // 派发重试任务（与 create_build_job 相同的调度链路）
    match dispatch_retry_task(&state, new_job_id, &project_name, &original, auth.user_id).await {
        Ok(()) => {
            info!(job_id = %new_job_id, "Retry build task dispatched");
        }
        Err(e) => {
            // 与创建路径一致：派发失败不阻塞响应，作业停留在 pending，
            // 由状态查询暴露（不留孤儿父作业——事务已保证两行都在）
            error!(error = %e, job_id = %new_job_id, "Failed to dispatch retry build task");
        }
    }

    // 记录审计日志
    let _ = state
        .audit_service
        .log_action(AuditLogParams {
            subject_id: auth.user_id,
            subject_type: "user",
            subject_name: None,
            action: crate::services::audit_service::AuditAction::BuildRetry.as_str(),
            resource_type: "build",
            resource_id: Some(new_job_id),
            resource_name: None,
            changes: Some(serde_json::json!({"original_job_id": id})),
            changes_summary: None,
            source_ip: None,
            user_agent: None,
            trace_id: None,
            result: "success",
            error_message: None,
        })
        .await;

    info!(
        original_job_id = %id,
        new_job_id = %new_job_id,
        "Build job retry created"
    );

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": new_job_id,
            "project_name": project_name,
            "status": "pending",
            "retry_of": id
        })),
    ))
}

// ==================== 辅助函数 ====================

/// build_jobs.build_type 枚举的合法取值（migrations/000004）
const VALID_BUILD_TYPES: [&str; 5] = ["node", "java", "rust", "frontend", "other"];

/// build_jobs.runner_capability 枚举的合法取值（migrations/000004）。
/// 该枚举与 build_type 是两个独立 ENUM：无 "other"，另有 docker/general。
/// 创建路径曾把 build_type 原样镜像过去，"other" 会让 CAST 直接 500。
const VALID_RUNNER_CAPABILITIES: [&str; 4] = ["node", "java", "rust", "frontend"];

/// build_type -> runner_capability 镜像映射：不在交集内的类型归入 general
fn runner_capability_for(build_type: &str) -> String {
    if VALID_RUNNER_CAPABILITIES.contains(&build_type) {
        build_type.to_string()
    } else {
        "general".to_string()
    }
}

/// 创建构建作业请求校验。
/// 此前 build_type 等非法值会直达 `CAST(... AS build_type)` 抛 500，
/// 这里提前给出 400。
fn validate_build_request(request: &CreateBuildJobRequest) -> Result<()> {
    // Pipeline-as-code：steps 为空时进入管道模式（Runner 从仓库读文件）
    if request.steps.is_empty() && request.pipeline_file.is_none() {
        return Err(AppError::validation(
            "Build job must have at least one step, or set pipeline_file to run a pipeline from the repository",
        ));
    }
    if !request.steps.is_empty() && request.pipeline_file.is_some() {
        return Err(AppError::validation(
            "steps and pipeline_file are mutually exclusive",
        ));
    }
    if request.steps.is_empty() {
        // 管道模式：校验文件路径合法性（Runner 侧还会做内容校验）
        let f = request.pipeline_file.as_deref().unwrap_or_default();
        if f.is_empty() || f.len() > 255 || f.split('/').any(|c| c == "..") || f.starts_with('/') {
            return Err(AppError::validation(
                "pipeline_file must be a relative path without '..' segments",
            ));
        }
    }
    if !VALID_BUILD_TYPES.contains(&request.build_type.as_str()) {
        return Err(AppError::validation(&format!(
            "Invalid build_type '{}'. Must be one of: {}",
            request.build_type,
            VALID_BUILD_TYPES.join(", ")
        )));
    }
    if request.project_name.is_empty() || request.project_name.len() > 255 {
        return Err(AppError::validation(
            "project_name must be 1-255 characters",
        ));
    }
    if request.repository_url.is_empty() || request.repository_url.len() > 2000 {
        return Err(AppError::validation(
            "repository_url must be 1-2000 characters",
        ));
    }
    if request.branch.is_empty() || request.branch.starts_with('-') {
        return Err(AppError::validation(
            "branch must be non-empty and must not start with '-'",
        ));
    }
    if let Some(ts) = request.timeout_secs
        && !(10..=86400).contains(&ts)
    {
        return Err(AppError::validation(
            "timeout_secs must be between 10 and 86400",
        ));
    }
    for step in &request.steps {
        if step.id.is_empty() || step.id.len() > 255 {
            return Err(AppError::validation(
                "each step id must be 1-255 characters",
            ));
        }
        // step.id 会成为 runner 侧脚本文件名/容器名的一部分，
        // 只允许安全字符（runner 侧也会再消毒一次）
        if !step
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(AppError::validation(
                "step id may only contain letters, digits, '.', '_' and '-'",
            ));
        }
    }
    Ok(())
}

/// 解析步骤类型
fn parse_step_type(step_type: &str) -> StepType {
    match step_type.to_lowercase().as_str() {
        "command" => StepType::Command,
        "script" => StepType::Script,
        "install" => StepType::Install,
        "build" => StepType::Build,
        "test" => StepType::Test,
        "package" => StepType::Package,
        "publish" => StepType::Publish,
        custom => StepType::Custom(custom.to_string()),
    }
}

/// 派发构建任务到 RabbitMQ
///
/// 使用 RunnerScheduler 选择合适的 Runner，然后派发任务到 RabbitMQ
async fn dispatch_build_task(
    state: &Arc<AppState>,
    task: &BuildTaskMessage,
    build_type: &str,
) -> Result<()> {
    // 使用 RunnerScheduler 选择合适的 Runner
    let schedule_result = state
        .runner_scheduler
        .schedule_build(build_type, &[])
        .await
        .map_err(|e| {
            error!(error = %e, build_type = %build_type, "Failed to schedule build task");
            AppError::internal_error(&format!("Failed to schedule build: {}", e))
        })?;

    info!(
        runner_id = %schedule_result.runner_id,
        runner_name = %schedule_result.runner_name,
        routing_key = %schedule_result.routing_key,
        job_id = %task.job_id,
        "Build task scheduled"
    );

    let payload = serde_json::to_vec(task).map_err(|e| {
        error!(error = %e, "Failed to serialize build task");
        AppError::internal_error("Failed to serialize build task")
    })?;

    // 获取发布器
    let publisher = state.rabbitmq_publisher.get().await.map_err(|e| {
        error!(error = %e, "Failed to get RabbitMQ publisher");
        AppError::internal_error("Failed to get RabbitMQ publisher")
    })?;

    // 使用调度返回的 runner_name 发布任务（定向派发）
    // 路由键：build.<capability>.<runner_name>。<capability> 必须落在 runner
    // 的队列绑定集内（它只绑定自身 capability 键），因此非能力型 build_type
    // （如 "other"）必须经镜像映射为 "general"，否则消息会被交换机静默丢弃
    // （端到端实测发现：build.other.demo-runner-01 无任何绑定匹配）
    let routing_type = runner_capability_for(build_type);
    publisher
        .publish_build_task(&routing_type, Some(&schedule_result.runner_name), &payload)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to publish build task");
            AppError::internal_error("Failed to publish build task")
        })?;

    // 注意：容量计数已在 schedule_build 的原子认领中 +1，
    // 此处不再重复递增（此前双重计数导致 runner 被超派）

    // 记录派发目标 runner（可观测性；取消走广播不依赖此列）
    if let Err(e) = sqlx::query("UPDATE build_jobs SET runner_name = $2 WHERE id = $1")
        .bind(schedule_result.runner_id)
        .bind(&schedule_result.runner_name)
        .execute(&state.db)
        .await
    {
        warn!(error = %e, job_id = %task.job_id, "Failed to record dispatched runner name");
    }

    Ok(())
}

/// 重建重试任务的 BuildTaskMessage 并按原链路派发
async fn dispatch_retry_task(
    state: &Arc<AppState>,
    job_id: Uuid,
    project_name: &str,
    original: &OriginalJob,
    triggered_by: Uuid,
) -> Result<()> {
    let params = &original.build_parameters;
    let steps_request: Vec<BuildStepRequest> = params
        .get("steps")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let parameters: std::collections::HashMap<String, serde_json::Value> = params
        .get("parameters")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let env_vars: std::collections::HashMap<String, String> = params
        .get("env_vars")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let timeout_secs: Option<i32> = params
        .get("timeout_secs")
        .and_then(|v| v.as_i64().map(|n| n as i32));
    let pipeline_file: Option<String> = params
        .get("pipeline_file")
        .and_then(|v| v.as_str().map(String::from));

    let task_id = Uuid::new_v4();
    let build_task = BuildTaskMessage {
        task_id,
        job_id,
        project: ProjectInfo {
            name: project_name.to_string(),
            repository_url: original.repository.clone(),
            branch: original.branch.clone(),
            commit: original.commit_hash.clone(),
            triggered_by,
        },
        build: BuildParameters {
            build_type: original.build_type.clone(),
            env_vars,
            parameters,
            timeout_secs: timeout_secs.map(|t| t.max(0) as u64),
        },
        steps: steps_request
            .iter()
            .map(|s| BuildStep {
                id: s.id.clone(),
                name: s.name.clone(),
                step_type: parse_step_type(&s.step_type),
                command: s.command.clone(),
                script: s.script.clone(),
                working_dir: s.working_dir.clone(),
                timeout_secs: s.timeout_secs,
                continue_on_failure: s.continue_on_failure,
                produces_artifact: s.produces_artifact,
                docker_image: s.docker_image.clone(),
            })
            .collect(),
        publish_target: None,
        pipeline_file,
    };

    dispatch_build_task(state, &build_task, &original.build_type).await
}

/// 从 build_jobs 行解构的重试源数据（与 create_build_job 相同的列集）
#[derive(sqlx::FromRow)]
struct OriginalJob {
    repository: String,
    branch: String,
    commit_hash: String,
    build_type: String,
    build_parameters: serde_json::Value,
    runner_capability: String,
    triggered_by: Uuid,
    status: String,
    tags: Option<serde_json::Value>,
    job_id: Option<Uuid>,
}
