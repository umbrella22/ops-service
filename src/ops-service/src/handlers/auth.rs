//! 认证相关的 HTTP 处理器

use crate::{
    auth::middleware::AuthContext, error::AppError, middleware::AppState, models::auth::*,
    services::audit_service::AuditAction,
};
use axum::{Json, extract::State, http::HeaderMap, response::IntoResponse};
use serde_json::json;
use std::sync::Arc;

/// 登录
pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    client_ip: crate::middleware::ClientIp,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    // ClientIp 由限流中间件按 trust_proxy 解析后注入（代理头只在
    // trust_proxy=true 时被信任），此处只做字符串化
    let client_ip = client_ip.0.to_string();
    let _ = headers;

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let response = state
        .auth_service
        .login(req, &client_ip, user_agent.as_deref())
        .await?;

    Ok(Json(response))
}

/// 刷新令牌
pub async fn refresh_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    client_ip: crate::middleware::ClientIp,
    Json(req): Json<RefreshTokenRequest>,
) -> Result<impl IntoResponse, AppError> {
    let client_ip = client_ip.0.to_string();
    let _ = headers;

    let token_pair = state.auth_service.refresh_token(req, &client_ip).await?;

    Ok(Json(token_pair))
}

/// 登出
pub async fn logout(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
    Json(req): Json<LogoutRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .auth_service
        .logout(&req.refresh_token, auth_context.user_id)
        .await?;

    // 审计日志（令牌已撤销成功，审计失败不应把登出变成 500）
    if let Err(e) = state
        .audit_service
        .log_action_simple(
            auth_context.user_id,
            AuditAction::UserLogout,
            Some("session"),
            Some(auth_context.user_id),
            Some("User logged out"),
            None,
        )
        .await
    {
        tracing::error!(error = %e, user_id = %auth_context.user_id, "Failed to write audit log for logout");
    }

    Ok(Json(json!({"message": "已成功登出"})))
}

/// 从所有设备登出
pub async fn logout_all(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
) -> Result<impl IntoResponse, AppError> {
    let revoked_count = state.auth_service.logout_all(auth_context.user_id).await?;

    // 审计日志（会话已全部撤销成功，审计失败不应把登出变成 500）
    if let Err(e) = state
        .audit_service
        .log_action_simple(
            auth_context.user_id,
            AuditAction::UserLogout,
            Some("session"),
            Some(auth_context.user_id),
            Some(&format!("Logged out from all devices, revoked {} sessions", revoked_count)),
            None,
        )
        .await
    {
        tracing::error!(error = %e, user_id = %auth_context.user_id, "Failed to write audit log for logout-all");
    }

    Ok(Json(json!({
        "message": format!("已从 {} 个设备登出", revoked_count)
    })))
}

/// 获取当前用户信息
pub async fn get_current_user(auth_context: AuthContext) -> Result<impl IntoResponse, AppError> {
    Ok(Json(json!({
        "id": auth_context.user_id,
        "username": auth_context.username,
        "roles": auth_context.roles,
        "scopes": auth_context.scopes,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;


}
