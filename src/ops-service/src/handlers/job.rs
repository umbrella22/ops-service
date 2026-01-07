//! Job API handlers
//! P2 阶段：作业相关的API端点

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::{auth::middleware::AuthContext, error::Result, middleware::AppState, models::job::*};

/// 创建命令作业
pub async fn create_command_job(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Json(request): Json<CreateCommandJobRequest>,
) -> Result<impl IntoResponse> {
    let job = state
        .job_service
        .create_command_job(request, auth_context.user_id)
        .await?;

    Ok((StatusCode::CREATED, Json(job)))
}

/// 查询作业详情
pub async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    _auth_context: AuthContext,
) -> Result<impl IntoResponse> {
    let job = state.job_service.get_job(job_id).await?;
    Ok(Json(job))
}

/// 查询作业列表
pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
    Query(filters): Query<JobListFilters>,
    _auth_context: AuthContext,
) -> Result<impl IntoResponse> {
    let jobs = state.job_service.list_jobs(filters).await?;
    Ok(Json(jobs))
}

/// 获取作业的任务列表
pub async fn get_job_tasks(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    _auth_context: AuthContext,
) -> Result<impl IntoResponse> {
    let tasks = state.job_service.get_job_tasks(job_id).await?;
    Ok(Json(tasks))
}

/// 取消作业
pub async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    auth_context: AuthContext,
    Json(request): Json<CancelJobRequest>,
) -> Result<impl IntoResponse> {
    state
        .job_service
        .cancel_job(job_id, auth_context.user_id, request.reason)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// 重试作业
pub async fn retry_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    auth_context: AuthContext,
    Json(request): Json<RetryJobRequest>,
) -> Result<impl IntoResponse> {
    let job = state
        .job_service
        .retry_job(job_id, request, auth_context.user_id)
        .await?;

    Ok(Json(job))
}

/// 获取作业统计
pub async fn get_job_statistics(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    _auth_context: AuthContext,
) -> Result<impl IntoResponse> {
    let stats = state.job_service.get_job_statistics(job_id).await?;
    Ok(Json(stats))
}
