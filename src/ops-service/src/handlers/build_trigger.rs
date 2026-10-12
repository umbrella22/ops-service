//! Git push 自动触发构建 (P4)
//!
//! 外部 Git 服务（GitHub/GitLab）push 时调用 webhook，按触发器配置
//! （仓库 + 分支匹配，签名验证）自动创建管道模式构建作业。
//!
//! 验签方式：
//! - github: `X-Hub-Signature-256: sha256=HMAC_SHA256(secret, body)`
//! - gitlab: `X-Gitlab-Token: <secret>`（常数时间比较）
//!
//! 安全说明：webhook 端点不经过 JWT（公开路由），完全依赖签名；
//! 构建参数（仓库/分支白名单/项目名/构建类型）全部来自触发器配置，
//! payload 仅提取分支名与 commit，不信任任何其他内容。

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    auth::middleware::AuthContext,
    error::{AppError, Result},
    handlers::build::create_build_job_core,
    middleware::AppState,
    services::audit_service::AuditAction,
};

// ==================== 常数时间比较 ====================

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ==================== 触发器 CRUD ====================

#[derive(Debug, Deserialize)]
pub struct CreateTriggerRequest {
    pub name: String,
    /// webhook 签名密钥（写后不可读）
    pub secret: String,
    /// github | gitlab
    #[serde(default = "default_provider")]
    pub provider: String,
    pub repository_url: String,
    /// 精确分支名或 `*`（任意分支）
    #[serde(default = "default_branch")]
    pub branch: String,
    pub project_name: String,
    #[serde(default = "default_build_type")]
    pub build_type: String,
    /// 管道文件路径（缺省 .ops.yml；构建以管道模式运行）
    #[serde(default)]
    pub pipeline_file: Option<String>,
    #[serde(default)]
    pub env_vars: std::collections::HashMap<String, String>,
}

fn default_provider() -> String {
    "github".to_string()
}
fn default_branch() -> String {
    "*".to_string()
}
fn default_build_type() -> String {
    "other".to_string()
}

fn validate_trigger(req: &CreateTriggerRequest) -> Result<()> {
    if req.name.is_empty() || req.name.len() > 100 {
        return Err(AppError::validation("name must be 1-100 characters"));
    }
    if req.secret.len() < 16 {
        return Err(AppError::validation("secret must be at least 16 characters"));
    }
    if !matches!(req.provider.as_str(), "github" | "gitlab") {
        return Err(AppError::validation("provider must be github or gitlab"));
    }
    if req.repository_url.is_empty() || req.repository_url.len() > 500 {
        return Err(AppError::validation(
            "repository_url must be 1-500 characters",
        ));
    }
    if req.branch.is_empty() || req.branch.len() > 255 {
        return Err(AppError::validation("branch must be 1-255 characters or '*'"));
    }
    if req.project_name.is_empty() || req.project_name.len() > 255 {
        return Err(AppError::validation(
            "project_name must be 1-255 characters",
        ));
    }
    const VALID: [&str; 5] = ["node", "java", "rust", "frontend", "other"];
    if !VALID.contains(&req.build_type.as_str()) {
        return Err(AppError::validation(&format!(
            "build_type must be one of: {}",
            VALID.join(", ")
        )));
    }
    if let Some(f) = &req.pipeline_file
        && (f.is_empty() || f.len() > 255 || f.split('/').any(|c| c == "..") || f.starts_with('/'))
    {
        return Err(AppError::validation(
            "pipeline_file must be a relative path without '..' segments",
        ));
    }
    Ok(())
}

/// 列出触发器（secret 永不回显）
pub async fn list_triggers(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "read", None, None)
        .await?;

    #[derive(sqlx::FromRow, Serialize)]
    struct TriggerRow {
        id: Uuid,
        name: String,
        provider: String,
        repository_url: String,
        branch: String,
        project_name: String,
        build_type: String,
        pipeline_file: Option<String>,
        env_vars: serde_json::Value,
        enabled: bool,
        last_triggered_at: Option<chrono::DateTime<chrono::Utc>>,
        trigger_count: i64,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    let rows: Vec<TriggerRow> = sqlx::query_as(
        "SELECT id, name, provider, repository_url, branch, project_name, build_type::text AS build_type, pipeline_file, env_vars, enabled, last_triggered_at, trigger_count, created_at FROM build_triggers ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to list build triggers");
        AppError::database("Failed to list build triggers")
    })?;

    Ok(Json(serde_json::json!({ "triggers": rows, "total": rows.len() })))
}

/// 创建触发器
pub async fn create_trigger(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "write", None, None)
        .await?;
    validate_trigger(&req)?;

    let secret_enc = crate::services::crypto_service::encrypt(&req.secret).await?;
    let id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO build_triggers (id, name, secret, provider, repository_url, branch, project_name, build_type, pipeline_file, env_vars, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, CAST($8 AS build_type), $9, $10, $11)",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&secret_enc)
    .bind(&req.provider)
    .bind(&req.repository_url)
    .bind(&req.branch)
    .bind(&req.project_name)
    .bind(&req.build_type)
    .bind(&req.pipeline_file)
    .bind(serde_json::to_value(&req.env_vars).unwrap_or_default())
    .bind(auth.user_id)
    .execute(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to create build trigger");
        AppError::database("Failed to create build trigger")
    })?;

    let _ = state
        .audit_service
        .log_action_simple(
            auth.user_id,
            AuditAction::BuildCreate,
            Some("build_trigger"),
            Some(id),
            Some(&format!("Created build trigger: {}", req.name)),
            None,
        )
        .await;

    info!(trigger_id = %id, name = %req.name, "Build trigger created");
    Ok((StatusCode::CREATED, Json(serde_json::json!({"id": id}))))
}

/// 更新触发器（secret 传空表示不修改）
pub async fn update_trigger(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "write", None, None)
        .await?;
    validate_trigger(&req)?;

    // secret 为空串表示不修改（读取接口永不回显 secret，客户端传 "" 保留原值）
    let secret_enc = if req.secret.is_empty() {
        None
    } else {
        Some(crate::services::crypto_service::encrypt(&req.secret).await?)
    };

    let result = sqlx::query(
        "UPDATE build_triggers SET name = $2, secret = COALESCE($3, secret), provider = $4, repository_url = $5, branch = $6, project_name = $7, build_type = CAST($8 AS build_type), pipeline_file = $9, env_vars = $10, updated_at = NOW() WHERE id = $1",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&secret_enc)
    .bind(&req.provider)
    .bind(&req.repository_url)
    .bind(&req.branch)
    .bind(&req.project_name)
    .bind(&req.build_type)
    .bind(&req.pipeline_file)
    .bind(serde_json::to_value(&req.env_vars).unwrap_or_default())
    .execute(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to update build trigger");
        AppError::database("Failed to update build trigger")
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Build trigger not found"));
    }

    let _ = state
        .audit_service
        .log_action_simple(
            auth.user_id,
            AuditAction::BuildCreate,
            Some("build_trigger"),
            Some(id),
            Some(&format!("Updated build trigger: {}", req.name)),
            None,
        )
        .await;

    Ok(Json(serde_json::json!({"id": id, "updated": true})))
}

/// 删除触发器
pub async fn delete_trigger(
    State(state): State<Arc<AppState>>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state
        .permission_service
        .require_permission(auth.user_id, "build", "write", None, None)
        .await?;

    let result = sqlx::query("DELETE FROM build_triggers WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to delete build trigger");
            AppError::database("Failed to delete build trigger")
        })?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("Build trigger not found"));
    }

    let _ = state
        .audit_service
        .log_action_simple(
            auth.user_id,
            AuditAction::BuildCreate,
            Some("build_trigger"),
            Some(id),
            Some("Deleted build trigger"),
            None,
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

// ==================== Webhook 处理 ====================

#[derive(Debug, sqlx::FromRow)]
struct TriggerRecord {
    id: Uuid,
    name: String,
    secret: String,
    provider: String,
    repository_url: String,
    branch: String,
    project_name: String,
    build_type: String,
    pipeline_file: Option<String>,
    env_vars: serde_json::Value,
    created_by: Uuid,
}

/// 从 push payload 提取 (branch, commit)；不认识的 payload 返回 None
fn extract_branch_commit(provider: &str, body: &Value) -> Option<(String, String)> {
    let r#ref = body.get("ref")?.as_str()?.to_string();
    let branch = r#ref.strip_prefix("refs/heads/")?.to_string();
    if branch.is_empty() {
        return None;
    }
    // commit：github 用 after；gitlab 用 checkout_sha（兜底 after）
    let commit = body
        .get("checkout_sha")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| body.get("after").and_then(|v| v.as_str()))
        .unwrap_or_default()
        .to_string();
    let _ = provider;
    Some((branch, commit))
}

/// Git push webhook：验签 → 分支匹配 → 自动创建管道模式构建
pub async fn git_push_webhook(
    State(state): State<Arc<AppState>>,
    Path(trigger_id): Path<Uuid>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse> {
    // 1) 加载触发器
    let trigger: TriggerRecord = sqlx::query_as(
        "SELECT id, name, secret, provider, repository_url, branch, project_name, build_type::text AS build_type, pipeline_file, env_vars, created_by FROM build_triggers WHERE id = $1 AND enabled = TRUE",
    )
    .bind(trigger_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to load build trigger");
        AppError::database("Failed to load build trigger")
    })?
    .ok_or_else(|| AppError::not_found("Build trigger not found"))?;

    // 2) 验签（先验签再解析，避免对未授权请求做任何解析工作）
    let secret = crate::services::crypto_service::decrypt(&trigger.secret).await?;
    match trigger.provider.as_str() {
        "github" => {
            let expected_sig = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("sha256="))
                .ok_or_else(|| {
                    AppError::authentication("Missing X-Hub-Signature-256 header")
                })?;
            // GitHub 规范：HMAC-SHA256(secret, body)，复用 webhook 中间件的实现
            let computed = crate::middleware::webhook_hmac::compute_hmac_signature_hex(
                secret.as_bytes(),
                &body,
            );
            if !constant_time_eq(computed.as_bytes(), expected_sig.as_bytes()) {
                warn!(trigger = %trigger.name, "Git webhook signature mismatch");
                return Err(AppError::authentication("Invalid webhook signature"));
            }
        }
        "gitlab" => {
            let token = headers
                .get("x-gitlab-token")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| AppError::authentication("Missing X-Gitlab-Token header"))?;
            if !constant_time_eq(token.as_bytes(), secret.as_bytes()) {
                warn!(trigger = %trigger.name, "Git webhook token mismatch");
                return Err(AppError::authentication("Invalid webhook token"));
            }
        }
        other => {
            return Err(AppError::validation(&format!(
                "Unknown provider config: {other}"
            )));
        }
    }

    // 3) 解析 payload（仅提取分支与 commit）
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| AppError::validation("Webhook body is not valid JSON"))?;

    // GitHub ping 事件：验签通过即确认 hook 配置成功
    if headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|e| e == "ping")
    {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "message": "pong"})),
        ));
    }

    let Some((branch, commit)) = extract_branch_commit(&trigger.provider, &payload) else {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ignored": "not a branch push payload"})),
        ));
    };

    // 删除分支的 push（after 全 0）：忽略
    if commit.chars().all(|c| c == '0') && !commit.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ignored": "branch deleted"})),
        ));
    }

    // 4) 分支匹配
    if trigger.branch != "*" && trigger.branch != branch {
        info!(
            trigger = %trigger.name,
            branch = %branch,
            "Push ignored: branch does not match trigger"
        );
        return Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ignored": "branch not matched", "branch": branch})),
        ));
    }

    // 5) 以触发器配置创建管道模式构建（参数全部来自触发器，不信任 payload）
    let request = crate::handlers::build::CreateBuildJobRequest {
        project_name: trigger.project_name.clone(),
        repository_url: trigger.repository_url.clone(),
        branch: branch.clone(),
        commit: (!commit.is_empty()).then_some(commit),
        build_type: trigger.build_type.clone(),
        env_vars: trigger
            .env_vars
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        parameters: Default::default(),
        steps: Vec::new(), // 管道模式：Runner 从仓库读 pipeline_file
        pipeline_file: Some(
            trigger
                .pipeline_file
                .clone()
                .unwrap_or_else(|| common::pipeline::DEFAULT_PIPELINE_FILE.to_string()),
        ),
        publish_target: None,
        timeout_secs: Some(1800),
        tags: Some(vec![format!("trigger:{}", trigger.name)]),
    };

    let response = create_build_job_core(&state, request, trigger.created_by).await?;
    let job_id = response.id;

    // 6) 更新触发统计
    let _ = sqlx::query(
        "UPDATE build_triggers SET last_triggered_at = NOW(), trigger_count = trigger_count + 1 WHERE id = $1",
    )
    .bind(trigger.id)
    .execute(&state.db)
    .await;

    info!(
        trigger = %trigger.name,
        branch = %branch,
        job_id = %job_id,
        "Build auto-triggered by git push"
    );

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "job_id": job_id,
            "branch": branch,
            "project": trigger.project_name,
        })),
    ))
}
