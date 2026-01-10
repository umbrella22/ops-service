use ops_service::{
    concurrency::ConcurrencyController,
    config::AppConfig,
    db,
    handlers::build_webhook::BuildMessageConsumer,
    handlers::health,
    middleware::{AppState, IpRateLimiter, RateLimitConfig},
    rabbitmq::{RabbitMqConsumer, RabbitMqPublisherPool},
    realtime::EventBus,
    routes,
    services::{RunnerScheduler, StorageService},
    telemetry,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::RwLock;

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

    // 初始化 IP 限流器
    let rate_limiter = std::sync::Arc::new(IpRateLimiter::new(RateLimitConfig::default()));

    let approval_service = std::sync::Arc::new(ops_service::services::ApprovalService::new(
        db_pool.clone(),
        audit_service.clone(),
        event_bus.clone(),
    ));

    // 初始化 RabbitMQ 发布器池
    let rabbitmq_publisher =
        std::sync::Arc::new(RabbitMqPublisherPool::new(config.rabbitmq.clone()));

    // 初始化 Runner Docker 配置缓存
    let runner_docker_config_cache = std::sync::Arc::new(RwLock::new(config.runner_docker.clone()));

    // 初始化 Runner 调度服务
    let runner_scheduler = std::sync::Arc::new(RunnerScheduler::new(db_pool.clone()));

    // 初始化存储服务 (P2.1)
    let storage_service = std::sync::Arc::new(StorageService::from_env().unwrap_or_else(|e| {
        tracing::warn!("Failed to load storage config from env: {}. Using default.", e);
        StorageService::new(ops_service::services::StorageConfig::default())
    }));

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
        job_service: std::sync::Arc::new(
            ops_service::services::JobService::new(
                db_pool.clone(),
                concurrency_controller.clone(),
                audit_service.clone(),
                config.ssh.clone(),
            )
            .with_event_bus(event_bus.clone()),
        ),
        approval_service,
        event_bus,
        concurrency_controller,
        rate_limiter,
        rabbitmq_publisher,
        runner_docker_config_cache,
        runner_scheduler,
        storage_service,
    });

    let app = routes::create_router(app_state.clone());

    // 启动 RabbitMQ 消费者（P2.1：Runner 回传链路闭环）
    let consumer_handle = start_rabbitmq_consumer(app_state.clone()).await;

    let addr = &config.server.addr;
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(addr = %addr, "Server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(
            config.server.graceful_shutdown_timeout_secs,
            consumer_handle,
        ))
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// 启动 RabbitMQ 消费者后台任务
async fn start_rabbitmq_consumer(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    let consumer_state = state.clone();
    tokio::spawn(async move {
        // 尝试创建消费者（如果 RabbitMQ 未配置，则跳过）
        let consumer = match RabbitMqConsumer::new(consumer_state.config.rabbitmq.clone()).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create RabbitMQ consumer: {}. RabbitMQ message consumption disabled.", e);
                return;
            }
        };

        // 设置消费队列
        if let Err(e) = consumer.setup_consumer_queues().await {
            tracing::warn!(
                "Failed to setup consumer queues: {}. RabbitMQ message consumption disabled.",
                e
            );
            return;
        }

        tracing::info!("RabbitMQ consumer started, listening for build status and log messages");

        let msg_consumer = BuildMessageConsumer::new(consumer_state);

        // 启动状态消息消费者
        let status_consumer = consumer.clone();
        let status_msg_consumer = msg_consumer.clone();
        let status_handle = tokio::spawn(async move {
            if let Err(e) = status_consumer
                .consume_status_messages(move |data| {
                    let consumer = status_msg_consumer.clone();
                    tokio::spawn(async move {
                        if let Err(e) = consumer.handle_status_message(data).await {
                            tracing::error!("Failed to handle status message: {}", e);
                        }
                    });
                })
                .await
            {
                tracing::error!("Status consumer error: {}", e);
            }
        });

        // 启动日志消息消费者
        let log_msg_consumer = msg_consumer.clone();
        let log_handle = tokio::spawn(async move {
            if let Err(e) = consumer
                .consume_log_messages(move |data| {
                    let consumer = log_msg_consumer.clone();
                    tokio::spawn(async move {
                        if let Err(e) = consumer.handle_log_message(data).await {
                            tracing::error!("Failed to handle log message: {}", e);
                        }
                    });
                })
                .await
            {
                tracing::error!("Log consumer error: {}", e);
            }
        });

        // 等待两个消费者完成（正常情况下不会完成）
        tokio::select! {
            _ = status_handle => {
                tracing::warn!("Status consumer stopped unexpectedly");
            }
            _ = log_handle => {
                tracing::warn!("Log consumer stopped unexpectedly");
            }
        }

        tracing::info!("RabbitMQ consumer stopped");
    })
}

async fn shutdown_signal(timeout_secs: u64, _consumer_handle: tokio::task::JoinHandle<()>) {
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

    // Note: consumer_handle will be aborted when the task is dropped
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
