//! 资产管理的 HTTP 处理器

use crate::{
    auth::middleware::AuthContext, error::AppError, middleware::AppState, models::asset::*,
};
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub environment: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

// ==================== Asset Groups ====================

/// 列出资产组
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "read", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let groups = repo.list_groups(query.environment.as_deref()).await?;

    Ok(Json(json!({
        "groups": groups,
        "count": groups.len()
    })))
}

/// 创建资产组
pub async fn create_group(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Json(req): Json<CreateGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let group = repo.create_group(&req, auth_context.user_id).await?;

    Ok(Json(json!({
        "message": "资产组创建成功",
        "group": group
    })))
}

/// 获取资产组详情
pub async fn get_group(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "read", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let group = repo.get_group(id).await?.ok_or_else(|| AppError::not_found("Resource not found"))?;

    Ok(Json(group))
}

/// 更新资产组
pub async fn update_group(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let group = repo
        .update_group(id, &req)
        .await?
        .ok_or_else(|| AppError::not_found("Resource not found"))?;

    Ok(Json(json!({
        "message": "资产组更新成功",
        "group": group
    })))
}

/// 删除资产组
pub async fn delete_group(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    repo.delete_group(id).await?;

    Ok(Json(json!({
        "message": "资产组删除成功"
    })))
}

// ==================== Hosts ====================

#[derive(Debug, Deserialize)]
pub struct HostListQuery {
    pub group_id: Option<Uuid>,
    pub environment: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub search: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

/// 列出主机
pub async fn list_hosts(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Query(query): Query<HostListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "read", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());

    let filters = HostListFilters {
        group_id: query.group_id,
        environment: query.environment,
        status: query.status,
        tags: query.tags,
        search: query.search,
    };

    let hosts = repo.list_hosts(&filters, query.limit, query.offset).await?;
    let count = repo.count_hosts(&filters).await?;

    Ok(Json(json!({
        "hosts": hosts,
        "count": hosts.len(),
        "total": count
    })))
}

/// 创建主机
pub async fn create_host(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Json(req): Json<CreateHostRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let host = repo.create_host(&req, auth_context.user_id).await?;

    Ok(Json(json!({
        "message": "主机创建成功",
        "host": host
    })))
}

/// 获取主机详情
pub async fn get_host(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "read", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let host = repo.get_host(id).await?.ok_or_else(|| AppError::not_found("Resource not found"))?;

    Ok(Json(host))
}

/// 更新主机
pub async fn update_host(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateHostRequest>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    let host = repo
        .update_host(id, &req, auth_context.user_id)
        .await?
        .ok_or_else(|| AppError::not_found("Resource not found"))?;

    Ok(Json(json!({
        "message": "主机更新成功",
        "host": host
    })))
}

/// 删除主机
pub async fn delete_host(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    // 检查权限
    state
        .permission_service
        .require_permission(auth_context.user_id, "asset", "write", None, None)
        .await?;

    let repo = crate::repository::AssetRepository::new(state.db.clone());
    repo.delete_host(id).await?;

    Ok(Json(json!({
        "message": "主机删除成功"
    })))
}
