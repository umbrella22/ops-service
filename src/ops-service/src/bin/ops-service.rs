use ops_service::{
    concurrency::ConcurrencyController, config::AppConfig, db, handlers::health,
    middleware::AppState, realtime::EventBus, routes, telemetry,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    if let Ok(path) = std::env::var("OPS_ENV") {
        dotenv::from_filename(format!(".env.{}", path)).ok();
    } else {
        dotenv::from_filename(".env.local").ok();
        dotenv::from_filename(".env.development").ok();
        dotenv::dotenv().ok();
    }

    health::set_start_time();

    let config = AppConfig::from_env().map_err(|e| {
        eprintln!("Configuration error: {}", e);
        anyhow::anyhow!("Failed to load configuration: {}", e)
    })?;

    telemetry::init_telemetry(&config);
    telemetry::init_metrics();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Ops System P0 starting...");

    let db_pool = db::create_pool(&config.database).await?;
    db::run_migrations(&db_pool).await?;

    tracing::info!("Database initialized");

    let concurrency_controller = std::sync::Arc::new(ConcurrencyController::new(
        ops_service::concurrency::ConcurrencyConfig::default(),
    ));

    let audit_service =
        std::sync::Arc::new(ops_service::services::AuditService::new(db_pool.clone()));

    let event_bus = std::sync::Arc::new(EventBus::new(1000));

    let approval_service = std::sync::Arc::new(ops_service::services::ApprovalService::new(
        db_pool.clone(),
        audit_service.clone(),
        event_bus.clone(),
    ));

    let app_state = Arc::new(AppState {
        db: db_pool.clone(),
        config: config.clone(),
        auth_service: std::sync::Arc::new(ops_service::services::AuthService::new(
            db_pool.clone(),
            std::sync::Arc::new(ops_service::auth::jwt::JwtService::from_config(&config)?),
            std::sync::Arc::new(config.clone()),
        )),
        permission_service: std::sync::Arc::new(ops_service::services::PermissionService::new(
            db_pool.clone(),
        )),
        audit_service: audit_service.clone(),
        jwt_service: std::sync::Arc::new(ops_service::auth::jwt::JwtService::from_config(&config)?),
        job_service: std::sync::Arc::new(ops_service::services::JobService::new(
            db_pool.clone(),
            concurrency_controller.clone(),
            audit_service,
            config.ssh.clone(),
        )),
        approval_service,
        event_bus,
    });

    let app = routes::create_router(app_state.clone());

    let addr = &config.server.addr;
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(addr = %addr, "Server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(config.server.graceful_shutdown_timeout_secs))
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

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

    tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)).await;
    tracing::warn!("Graceful shutdown timeout reached, forcing exit");
}

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
