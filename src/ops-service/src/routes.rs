//! 路由注册
//! 创建所有 API 路由并应用中间件

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    auth::JwtService,
    handlers,
    middleware::{AppState, IpRateLimiter, RateLimitConfig},
    rabbitmq::RabbitMqPublisherPool,
    realtime::EventBus,
    services::{
        ApprovalService, AuditService, AuthService, JobService, PermissionService, RunnerScheduler,
        StorageService,
    },
};

// 导出 runner_auth_middleware
use crate::middleware::runner_auth_middleware;

/// 创建应用路由
pub fn create_router(state: Arc<AppState>) -> Router {
    // 创建所有服务
    let jwt_service =
        Arc::new(JwtService::from_config(&state.config).expect("Failed to create JWT service"));

    let auth_service = Arc::new(AuthService::new(
        state.db.clone(),
        jwt_service.clone(),
        Arc::new(state.config.clone()),
    ));

    let permission_service = Arc::new(PermissionService::new(state.db.clone()));
    let audit_service = Arc::new(AuditService::new(state.db.clone()));

    // 创建并发控制器（从配置加载）
    let strategy = match state.config.concurrency.strategy.to_lowercase().as_str() {
        "reject" => crate::concurrency::ConcurrencyStrategy::Reject,
        "queue" => crate::concurrency::ConcurrencyStrategy::Queue,
        _ => crate::concurrency::ConcurrencyStrategy::Wait,
    };

    let concurrency_config = crate::concurrency::ConcurrencyConfig {
        global_limit: state.config.concurrency.global_limit,
        group_limit: state.config.concurrency.group_limit,
        environment_limit: state.config.concurrency.environment_limit,
        production_limit: state.config.concurrency.production_limit,
        acquire_timeout_secs: state.config.concurrency.acquire_timeout_secs,
        strategy,
        queue_max_length: state.config.concurrency.queue_max_length,
    };

    let concurrency_controller =
        std::sync::Arc::new(crate::concurrency::ConcurrencyController::new(concurrency_config));

    let job_service = Arc::new(JobService::new(
        state.db.clone(),
        concurrency_controller.clone(),
        audit_service.clone(),
        state.config.ssh.clone(),
    ));

    // 创建事件总线 (P3 实时能力)
    let event_bus = Arc::new(EventBus::new(1000));

    // 创建审批服务 (P3 审批流)
    let approval_service =
        Arc::new(ApprovalService::new(state.db.clone(), audit_service.clone(), event_bus.clone()));

    // 创建 RabbitMQ 发布器池 (P2.1)
    let rabbitmq_publisher = Arc::new(RabbitMqPublisherPool::new(state.config.rabbitmq.clone()));

    // 创建完整的 AppState
    // 初始化 Runner Docker 配置缓存
    let runner_docker_config_cache = Arc::new(RwLock::new(state.config.runner_docker.clone()));

    // 初始化 Runner 调度服务
    let runner_scheduler = Arc::new(RunnerScheduler::new(state.db.clone()));

    // 初始化存储服务
    let storage_service = Arc::new(StorageService::from_env().unwrap_or_else(|e| {
        tracing::warn!("Failed to load storage config from env: {}. Using default.", e);
        StorageService::new(crate::services::StorageConfig::default())
    }));

    let full_state = Arc::new(AppState {
        config: state.config.clone(),
        db: state.db.clone(),
        auth_service,
        permission_service,
        audit_service,
        jwt_service: jwt_service.clone(),
        job_service,
        approval_service,
        event_bus,
        concurrency_controller,
        rate_limiter: Arc::new(IpRateLimiter::new(RateLimitConfig::default())),
        rabbitmq_publisher,
        runner_docker_config_cache,
        runner_scheduler,
        storage_service,
    });

    // 公开端点（健康检查）
    let public_routes = Router::new()
        .route("/health", get(handlers::health::health_check))
        .route("/ready", get(handlers::health::readiness_check))
        .route("/api/v1/system/concurrency", get(handlers::health::get_concurrency_status));

    // Runner Webhook 路由（使用 Runner API Key 鉴权）
    let runner_routes = Router::new()
        // Runner 注册（兼容 ops-runner 调用路径）
        .route(
            "/api/v1/runners/register",
            post(handlers::runner::register_runner)
        )
        .route(
            "/api/v1/webhooks/runner/register",
            post(handlers::runner::register_runner)
        )
        // Runner 心跳（兼容 ops-runner 调用路径）
        .route(
            "/api/v1/runners/heartbeat",
            post(handlers::runner::runner_heartbeat)
        )
        .route(
            "/api/v1/webhooks/runner/heartbeat",
            post(handlers::runner::runner_heartbeat)
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            runner_auth_middleware,
        ));

    // 其他 Webhook 路由（公开，但应在反向代理层做 IP 白名单限制）
    let webhook_routes = Router::new()
        // 构建状态更新
        .route(
            "/api/v1/webhooks/build/status",
            post(handlers::build_webhook::build_status_webhook)
        )
        // 构建日志
        .route(
            "/api/v1/webhooks/build/log",
            post(handlers::build_webhook::build_log_webhook)
        )
        // 构建产物元数据
        .route(
            "/api/v1/webhooks/build/artifact",
            post(handlers::build_webhook::build_artifact_webhook)
        );

    // 认证路由（无需认证，但应用速率限制）
    let auth_routes = Router::new()
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh_token));

    // 需要认证的路由
    let authenticated_routes = Router::new()
        // 当前用户信息
        .route("/api/v1/auth/me", get(handlers::auth::get_current_user))
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        .route("/api/v1/auth/logout-all", post(handlers::auth::logout_all))

        // 用户管理（需要权限）
        .route(
            "/api/v1/users",
            get(handlers::user::list_users)
                .post(handlers::user::create_user)
        )
        .route(
            "/api/v1/users/{id}",
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
            "/api/v1/groups/{id}",
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
            "/api/v1/hosts/{id}",
            get(handlers::asset::get_host)
                .put(handlers::asset::update_host)
                .delete(handlers::asset::delete_host)
        )

        // 作业管理
        .route(
            "/api/v1/jobs",
            get(handlers::job::list_jobs)
        )
        .route(
            "/api/v1/jobs/command",
            post(handlers::job::create_command_job)
        )
        .route(
            "/api/v1/jobs/script",
            post(handlers::job::create_script_job)
        )
        .route(
            "/api/v1/jobs/{id}",
            get(handlers::job::get_job)
        )
        .route(
            "/api/v1/jobs/{id}/tasks",
            get(handlers::job::get_job_tasks)
        )
        .route(
            "/api/v1/jobs/{id}/cancel",
            post(handlers::job::cancel_job)
        )
        .route(
            "/api/v1/jobs/{id}/retry",
            post(handlers::job::retry_job)
        )
        .route(
            "/api/v1/jobs/{id}/statistics",
            get(handlers::job::get_job_statistics)
        )

        // 审计日志（需要审计权限）
        .route("/api/v1/audit/logs", get(handlers::audit::list_audit_logs))
        .route("/api/v1/audit/login-events", get(handlers::audit::list_login_events))

        // 角色管理（P1）
        .route(
            "/api/v1/roles",
            get(handlers::role::list_roles)
                .post(handlers::role::create_role)
        )
        .route(
            "/api/v1/roles/{id}",
            get(handlers::role::get_role)
                .put(handlers::role::update_role)
                .delete(handlers::role::delete_role)
        )
        .route(
            "/api/v1/permissions",
            get(handlers::role::list_permissions)
        )
        .route(
            "/api/v1/roles/{id}/permissions",
            get(handlers::role::get_role_permissions)
        )
        .route(
            "/api/v1/role-bindings",
            post(handlers::role::assign_role)
        )
        .route(
            "/api/v1/role-bindings/{id}",
            delete(handlers::role::revoke_role)
        )
        .route(
            "/api/v1/users/{user_id}/roles",
            get(handlers::role::get_user_roles)
        )

        // 审批管理 (P3)
        .route(
            "/api/v1/approvals",
            get(handlers::approval::list_approval_requests)
                .post(handlers::approval::create_approval_request)
        )
        .route(
            "/api/v1/approvals/{id}",
            get(handlers::approval::get_approval_request)
        )
        .route(
            "/api/v1/approvals/{id}/approve",
            post(handlers::approval::approve_request)
        )
        .route(
            "/api/v1/approvals/{id}/cancel",
            post(handlers::approval::cancel_approval_request)
        )
        .route(
            "/api/v1/approval-groups",
            post(handlers::approval::create_approval_group)
        )
        .route(
            "/api/v1/approvals/statistics",
            get(handlers::approval::get_approval_statistics)
        )

        // 作业模板 (P3)
        .route(
            "/api/v1/job-templates",
            post(handlers::approval::create_job_template)
        )
        .route(
            "/api/v1/job-templates/execute",
            post(handlers::approval::execute_template_job)
        )

        // 实时事件流 (P3 - SSE)
        .route(
            "/api/v1/stream/approvals",
            get(handlers::approval::subscribe_approval_events)
        )
        .route(
            "/api/v1/stream/jobs/{id}",
            get(handlers::approval::subscribe_job_events)
        )

        // 构建作业 (P2.1)
        .route(
            "/api/v1/builds",
            post(handlers::build::create_build_job)
                .get(handlers::build::list_build_jobs)
        )
        .route(
            "/api/v1/builds/{id}",
            get(handlers::build::get_build_job)
        )
        .route(
            "/api/v1/builds/{id}/cancel",
            post(handlers::build::cancel_build_job)
        )
        .route(
            "/api/v1/builds/{id}/retry",
            post(handlers::build::retry_build_job)
        )
        .route(
            "/api/v1/builds/{id}/steps",
            get(handlers::build::get_build_steps)
        )

        // Runner 管理 (P2.1)
        .route(
            "/api/v1/runners",
            get(handlers::runner::list_runners)
        )
        .route(
            "/api/v1/runners/{id}",
            get(handlers::runner::get_runner_status)
                .put(handlers::runner::update_runner_status)
                .delete(handlers::runner::delete_runner)
        )

        // Runner Docker 配置管理 (Web UI)
        .route(
            "/api/v1/runner-docker-configs",
            get(handlers::runner_config::list_runner_configs)
                .post(handlers::runner_config::create_runner_config)
        )
        .route(
            "/api/v1/runner-docker-configs/{id}",
            get(handlers::runner_config::get_runner_config)
                .put(handlers::runner_config::update_runner_config)
                .delete(handlers::runner_config::delete_runner_config)
        )
        .route(
            "/api/v1/runner-docker-configs/{id}/history",
            get(handlers::runner_config::get_config_history)
        )

        // 构建产物 (P2.1)
        .route(
            "/api/v1/artifacts",
            post(handlers::artifact::record_artifact)
                .get(handlers::artifact::list_artifacts)
        )
        .route(
            "/api/v1/artifacts/{id}",
            get(handlers::artifact::get_artifact)
                .put(handlers::artifact::update_artifact)
                .delete(handlers::artifact::delete_artifact)
        )
        .route(
            "/api/v1/artifacts/{id}/download",
            post(handlers::artifact::record_download)
        )
        .route(
            "/api/v1/artifacts/{id}/download-url",
            get(handlers::artifact::generate_download_url)
        )
        .route(
            "/api/v1/artifacts/{id}/downloads",
            get(handlers::artifact::get_download_history)
        )
        .layer(axum::middleware::from_fn_with_state(
            jwt_service.clone(),
            crate::auth::middleware::jwt_auth_middleware,
        ));

    // 指标端点
    let metrics_routes = Router::new().route("/metrics", get(handlers::metrics::metrics_export));

    // 组合所有路由
    Router::new()
        .merge(public_routes)
        .merge(runner_routes)
        .merge(webhook_routes)
        .merge(auth_routes)
        .merge(authenticated_routes)
        .merge(metrics_routes)
        .layer(axum::middleware::from_fn_with_state(
            full_state.clone(),
            crate::middleware::ip_whitelist_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            full_state.clone(),
            crate::middleware::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(crate::middleware::request_tracking_middleware))
        .with_state(full_state)
}
