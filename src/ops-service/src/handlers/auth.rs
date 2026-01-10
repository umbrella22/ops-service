//! 认证相关的 HTTP 处理器

use crate::{
    auth::middleware::AuthContext, error::AppError, middleware::AppState, models::auth::*,
    services::audit_service::AuditAction,
};
use axum::{extract::State, http::HeaderMap, response::IntoResponse, Json};
use serde_json::json;
use std::sync::Arc;

/// 登录
pub async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let client_ip = get_client_ip(&headers).unwrap_or("unknown".to_string());
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
    Json(req): Json<RefreshTokenRequest>,
) -> Result<impl IntoResponse, AppError> {
    let client_ip = get_client_ip(&headers).unwrap_or("unknown".to_string());

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

    // 审计日志
    state
        .audit_service
        .log_action_simple(
            auth_context.user_id,
            AuditAction::UserLogout,
            Some("session"),
            Some(auth_context.user_id),
            Some("User logged out"),
            None,
        )
        .await?;

    Ok(Json(json!({"message": "已成功登出"})))
}

/// 从所有设备登出
pub async fn logout_all(
    State(state): State<Arc<AppState>>,
    auth_context: AuthContext,
) -> Result<impl IntoResponse, AppError> {
    let revoked_count = state.auth_service.logout_all(auth_context.user_id).await?;

    // 审计日志
    state
        .audit_service
        .log_action_simple(
            auth_context.user_id,
            AuditAction::UserLogout,
            Some("session"),
            Some(auth_context.user_id),
            Some(&format!("Logged out from all devices, revoked {} sessions", revoked_count)),
            None,
        )
        .await?;

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

/// 获取客户端 IP 地址
fn get_client_ip(headers: &HeaderMap) -> Option<String> {
    // 首先检查 X-Forwarded-For（代理情况）
    if let Some(forwarded) = headers.get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            // X-Forwarded-For 可能包含多个 IP，取第一个
            if let Some(first_ip) = forwarded_str.split(',').next() {
                return Some(first_ip.trim().to_string());
            }
        }
    }

    // 然后检查 X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            return Some(ip_str.to_string());
        }
    }

    // 最后使用连接的远程地址（在 handler 中通常由 Axum 自动处理）
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_client_ip_from_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "192.168.1.1, 10.0.0.1".parse().unwrap());

        let ip = get_client_ip(&headers);
        assert_eq!(ip, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn test_get_client_ip_from_x_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.168.1.2".parse().unwrap());

        let ip = get_client_ip(&headers);
        assert_eq!(ip, Some("192.168.1.2".to_string()));
    }
}
