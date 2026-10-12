use secrecy::ExposeSecret;
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

    // 主机凭据落盘加密密钥（未配置时退化为明文，config.validate 已输出警告）
    ops_service::services::crypto_service::init(
        config
            .security
            .credential_encryption_key
            .as_ref()
            .map(|k| k.expose_secret().to_string()),
    )
    .await;

    // 并发控制器：使用 config.concurrency 配置（此前被硬编码为 default，
    // 环境变量调优完全不生效）
    let concurrency_settings = &config.concurrency;
    let strategy = match concurrency_settings.strategy.to_lowercase().as_str() {
        "reject" => ops_service::concurrency::ConcurrencyStrategy::Reject,
        "queue" => ops_service::concurrency::ConcurrencyStrategy::Queue,
        _ => ops_service::concurrency::ConcurrencyStrategy::Wait,
    };
    let concurrency_controller = std::sync::Arc::new(ConcurrencyController::new(
        ops_service::concurrency::ConcurrencyConfig {
            global_limit: concurrency_settings.global_limit,
            group_limit: concurrency_settings.group_limit,
            environment_limit: concurrency_settings.environment_limit,
            production_limit: concurrency_settings.production_limit,
            acquire_timeout_secs: concurrency_settings.acquire_timeout_secs,
            strategy,
            queue_max_length: concurrency_settings.queue_max_length,
        },
    ));

    let audit_service =
        std::sync::Arc::new(ops_service::services::AuditService::new(db_pool.clone()));

    let event_bus = std::sync::Arc::new(EventBus::new(1000));

    // 初始化 IP 限流器（从 config.security.rate_limit_rps 加载配置）
    let rate_limiter = std::sync::Arc::new(IpRateLimiter::new(
        RateLimitConfig::from_security_config(&config.security),
    ));

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

    // 终态通知服务（未启用时为 None）
    let notify_service =
        ops_service::services::notify_service::NotifyService::from_config(&config.notifications);

    // 初始化 Webhook Nonce 防重放存储
    let webhook_nonce_store =
        std::sync::Arc::new(ops_service::middleware::webhook_hmac::NonceStore::new(
            config.security.runner_webhook_nonce_ttl_secs,
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
        job_service: std::sync::Arc::new(
            ops_service::services::JobService::new(
                db_pool.clone(),
                concurrency_controller.clone(),
                audit_service.clone(),
                config.ssh.clone(),
            )
            .with_event_bus(event_bus.clone())
            .with_approval_service(approval_service.clone()),
        ),
        approval_service,
        event_bus,
        concurrency_controller,
        rate_limiter,
        rabbitmq_publisher,
        runner_docker_config_cache,
        runner_scheduler,
        storage_service,
        webhook_nonce_store: Some(webhook_nonce_store),
        runner_config_version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        notify_service,
    });

    let app = routes::create_router(app_state.clone());

    // 启动 RabbitMQ 消费者（P2.1：Runner 回传链路闭环）
    let consumer_handle = start_rabbitmq_consumer(app_state.clone()).await;

    // 启动审批超时自动过期任务 (P3)
    let expiry_handle = start_approval_expiry_task(app_state.clone());

    // 启动限流器空闲回收任务（此前 cleanup_expired 从未被调度，
    // IP 状态只增不减；修复后按空闲精确回收）
    {
        let limiter = app_state.rate_limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                limiter.cleanup_expired(0).await;
            }
        });
    }

    let addr = &config.server.addr;
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(addr = %addr, "Server listening");

    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal(
            config.server.graceful_shutdown_timeout_secs,
            consumer_handle,
            expiry_handle,
        ))
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

/// 启动 RabbitMQ 消费者后台任务（真正的自动重连：连接断开后重建
/// RabbitMqConsumer，而不是对同一个死 channel 反复 basic_consume）
async fn start_rabbitmq_consumer(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    let consumer_state = state.clone();
    tokio::spawn(async move {
        let msg_consumer = BuildMessageConsumer::new(consumer_state.clone());

        loop {
            // 每轮循环都重新建立连接/通道/队列
            let consumer = match RabbitMqConsumer::new(consumer_state.config.rabbitmq.clone())
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        "Failed to create RabbitMQ consumer: {}. Retrying in 5s...",
                        e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if let Err(e) = consumer.setup_consumer_queues().await {
                tracing::warn!("Failed to setup consumer queues: {}. Retrying in 5s...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }

            tracing::info!("RabbitMQ consumer started, listening for build status and log messages");

            let status_msg_consumer = msg_consumer.clone();
            let status_cons = consumer.clone();
            let status_handle = tokio::spawn(async move {
                // 处理完成后才 ack；失败按 requeue / DLQ 策略由消费循环处理
                if let Err(e) = status_cons
                    .consume_status_messages(move |data, _delivery| {
                        let c = status_msg_consumer.clone();
                        async move { c.handle_status_message(data).await }
                    })
                    .await
                {
                    tracing::error!("Status consumer error: {}", e);
                }
            });

            let log_msg_consumer = msg_consumer.clone();
            let log_cons = consumer.clone();
            let log_handle = tokio::spawn(async move {
                if let Err(e) = log_cons
                    .consume_log_messages(move |data, _delivery| {
                        let c = log_msg_consumer.clone();
                        async move { c.handle_log_message(data).await }
                    })
                    .await
                {
                    tracing::error!("Log consumer error: {}", e);
                }
            });

            // 任一消费者退出（连接断开）即整轮重建
            tokio::select! {
                _ = status_handle => {
                    tracing::warn!("Status consumer stopped, reconnecting in 5s...");
                }
                _ = log_handle => {
                    tracing::warn!("Log consumer stopped, reconnecting in 5s...");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    })
}

async fn shutdown_signal(
    timeout_secs: u64,
    _consumer_handle: tokio::task::JoinHandle<()>,
    _expiry_handle: tokio::task::JoinHandle<()>,
) {
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

/// 审批超时自动过期后台任务
fn start_approval_expiry_task(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let result = sqlx::query(
                "UPDATE approval_requests SET status = 'timeout', completed_at = NOW()
                 WHERE status = 'pending' AND expires_at < NOW()",
            )
            .execute(&state.db)
            .await;

            match result {
                Ok(r) if r.rows_affected() > 0 => {
                    tracing::info!(rows = r.rows_affected(), "Auto-expired approval requests");
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Failed to auto-expire approvals");
                }
            }
        }
    })
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
