//! 运维系统主入口
//! P0 阶段：骨架与基线能力

use ops_system::{
    concurrency::ConcurrencyController,
    config::AppConfig, db, handlers::health, middleware::AppState, realtime::EventBus, routes, telemetry,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ===== CLI 参数处理 =====
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "--version" => {
                println!("ops-system {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" => {
                print_help();
                return Ok(());
            }
            _ => {
                eprintln!("未知参数: {}", args[1]);
                print_help();
                std::process::exit(1);
            }
        }
    }

    // 加载 .env 文件（开发环境）
    // 按优先级加载：.env.local > .env.development > .env
    // 生产环境应该直接设置环境变量，不依赖 .env 文件
    if let Ok(path) = std::env::var("OPS_ENV") {
        dotenv::from_filename(format!(".env.{}", path)).ok();
    } else {
        dotenv::from_filename(".env.local").ok();
        dotenv::from_filename(".env.development").ok();
        dotenv::dotenv().ok();
    }

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
    let concurrency_controller = std::sync::Arc::new(ConcurrencyController::new(
        ops_system::concurrency::ConcurrencyConfig::default()
    ));

    // 创建审计服务（多个服务共享）
    let audit_service = std::sync::Arc::new(ops_system::services::AuditService::new(
        db_pool.clone(),
    ));

    // 创建事件总线 (P3 实时能力)
    let event_bus = std::sync::Arc::new(EventBus::new(1000));

    // 创建审批服务 (P3 审批流)
    let approval_service = std::sync::Arc::new(ops_system::services::ApprovalService::new(
        db_pool.clone(),
        audit_service.clone(),
        event_bus.clone(),
    ));

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
        audit_service: audit_service.clone(),
        jwt_service: std::sync::Arc::new(ops_system::auth::jwt::JwtService::from_config(&config)?),
        job_service: std::sync::Arc::new(ops_system::services::JobService::new(
            db_pool.clone(),
            concurrency_controller.clone(),
            audit_service,
        )),
        approval_service,
        event_bus,
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

/// 打印帮助信息
fn print_help() {
    println!("ops-system {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("用法: ops-system [选项]");
    println!();
    println!("选项:");
    println!("  --version     打印版本信息并退出");
    println!("  --help        打印此帮助信息并退出");
    println!();
    println!("环境变量:");
    println!("  所有配置通过环境变量完成");
    println!("  可用选项请参考 .env.example");
}
