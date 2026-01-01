//! 运维系统主入口
//! P0 阶段：骨架与基线能力

use ops_system::{
    config::AppConfig, db, handlers::health, middleware::AppState, routes, telemetry,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 设置应用启动时间
    health::set_start_time();

    // 1. 加载配置
    let config = AppConfig::from_env().map_err(|e| {
        eprintln!("Configuration error: {}", e);
        anyhow::anyhow!("Failed to load configuration: {}", e)
    })?;

    // 2. 初始化日志与指标
    telemetry::init_telemetry(&config);
    telemetry::init_metrics();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Ops System P0 starting...");

    // 3. 数据库连接池 + 迁移
    let db_pool = db::create_pool(&config.database).await?;
    db::run_migrations(&db_pool).await?;

    tracing::info!("Database initialized");

    // 4. 构建应用状态
    // 注意: 这里的 AppState 只包含基础配置，services 会在 routes.rs 中创建
    let app_state = Arc::new(AppState {
        db: db_pool.clone(),
        config: config.clone(),
        auth_service: std::sync::Arc::new(ops_system::services::AuthService::new(
            db_pool.clone(),
            std::sync::Arc::new(ops_system::auth::jwt::JwtService::from_config(&config)?),
            std::sync::Arc::new(config.clone()),
        )),
        permission_service: std::sync::Arc::new(ops_system::services::PermissionService::new(
            db_pool.clone(),
        )),
        audit_service: std::sync::Arc::new(ops_system::services::AuditService::new(
            db_pool.clone(),
        )),
        jwt_service: std::sync::Arc::new(ops_system::auth::jwt::JwtService::from_config(&config)?),
    });

    // 5. 构建路由
    let app = routes::create_router(app_state.clone());

    // 6. 启动服务器
    let addr = &config.server.addr;
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(
        addr = %addr,
        "Server listening"
    );

    // 7. 优雅关闭
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(config.server.graceful_shutdown_timeout_secs))
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// 优雅关闭信号处理
async fn shutdown_signal(timeout_secs: u64) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Ctrl+C received, starting graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Terminate signal received, starting graceful shutdown");
        },
    }

    // 超时后强制关闭
    tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)).await;
    tracing::warn!("Graceful shutdown timeout reached, forcing exit");
}
