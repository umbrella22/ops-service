//! 路由注册
//! 创建所有 API 路由并应用中间件

use axum::{
    routing::{get, post, put},
    Router,
};
use std::sync::Arc;

use crate::{
    auth::JwtService,
    handlers,
    middleware::AppState,
    services::{AuthService, PermissionService, AuditService},
};

/// 创建应用路由
pub fn create_router(state: Arc<AppState>) -> Router {
    // 创建所有服务
    let jwt_service = Arc::new(JwtService::from_config(&state.config)
        .expect("Failed to create JWT service"));

    let auth_service = Arc::new(AuthService::new(
        state.db.clone(),
        jwt_service.clone(),
        Arc::new(state.config.clone()),
    ));

    let permission_service = Arc::new(PermissionService::new(state.db.clone()));
    let audit_service = Arc::new(AuditService::new(state.db.clone()));

    // 创建完整的 AppState
    let full_state = AppState {
        config: state.config.clone(),
        db: state.db.clone(),
        auth_service,
        permission_service,
        audit_service,
        jwt_service,
    };

    // 包装在 Arc 中
    let state = Arc::new(full_state);

    // 公开端点（健康检查）
    let public_routes = Router::new()
        .route("/health", get(handlers::health::health_check))
        .route("/ready", get(handlers::health::readiness_check));

    // 认证路由（无需认证，但应用速率限制）
    let auth_routes = Router::new()
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh_token))
        .route("/api/v1/auth/logout", post(handlers::auth::logout));

    // 需要认证的路由
    let authenticated_routes = Router::new()
        // 当前用户信息
        .route("/api/v1/auth/me", get(handlers::auth::get_current_user))
        .route("/api/v1/auth/logout-all", post(handlers::auth::logout_all))

        // 用户管理（需要权限）
        .route(
            "/api/v1/users",
            get(handlers::user::list_users)
                .post(handlers::user::create_user)
        )
        .route(
            "/api/v1/users/:id",
            get(handlers::user::get_user)
                .put(handlers::user::update_user)
                .delete(handlers::user::delete_user)
        )
        .route("/api/v1/users/me/password", put(handlers::user::change_password))

        // 资产组
        .route(
            "/api/v1/groups",
            get(handlers::asset::list_groups)
                .post(handlers::asset::create_group)
        )
        .route(
            "/api/v1/groups/:id",
            get(handlers::asset::get_group)
                .put(handlers::asset::update_group)
                .delete(handlers::asset::delete_group)
        )

        // 主机
        .route(
            "/api/v1/hosts",
            get(handlers::asset::list_hosts)
                .post(handlers::asset::create_host)
        )
        .route(
            "/api/v1/hosts/:id",
            get(handlers::asset::get_host)
                .put(handlers::asset::update_host)
                .delete(handlers::asset::delete_host)
        )

        // 审计日志（需要审计权限）
        .route("/api/v1/audit/logs", get(handlers::audit::list_audit_logs))
        .route("/api/v1/audit/login-events", get(handlers::audit::list_login_events));

    // 指标端点
    let metrics_routes = Router::new()
        .route("/metrics", get(handlers::metrics::metrics_export));

    // 组合所有路由
    Router::new()
        .merge(public_routes)
        .merge(auth_routes)
        .merge(authenticated_routes)
        .merge(metrics_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::ip_whitelist_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::request_tracking_middleware,
        ))
        .with_state(state)
}
