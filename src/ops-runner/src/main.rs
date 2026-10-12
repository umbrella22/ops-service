//! ops-runner - 构建作业执行代理
//!
//! ops-runner 是一个独立的代理程序，运行在构建机器上，负责执行构建作业。
//! 它通过 RabbitMQ 接收来自控制面的构建任务，执行构建步骤，并将结果回传。

mod client;
mod config;
mod docker;
mod executor;
mod messages;
mod publisher;
mod worker;

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use client::ControlPlaneClient;
use config::RunnerConfig;
use worker::TaskWorker;

/// ops-runner - 构建作业执行代理
#[derive(Parser, Debug)]
#[command(name = "ops-runner")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(author = "ops-system")]
#[command(about = "Build job execution agent for ops-system", long_about = None)]
struct Args {
    /// 配置文件路径（可选，未指定时从环境变量读取）
    #[arg(short, long)]
    config: Option<String>,

    /// 详细模式
    #[arg(short, long)]
    verbose: bool,
}

fn print_version() {
    println!("ops-runner {}", env!("CARGO_PKG_VERSION"));
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 初始化日志
    let log_level = if args.verbose {
        "debug".to_string()
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())
    };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::new(log_level))
        .init();

    info!("ops-runner starting...");
    print_version();

    // 加载配置
    let config = if let Some(path) = &args.config {
        info!("Loading config from file: {}", path);
        let content = std::fs::read_to_string(path)
            .context(format!("Failed to read config file: {}", path))?;
        match path.ends_with(".json") {
            true => serde_json::from_str(&content).context("Failed to parse JSON config")?,
            false => toml::from_str(&content).context("Failed to parse TOML config")?,
        }
    } else {
        RunnerConfig::from_env()?
    };

    info!("Runner name: {}", config.runner.name);
    info!("Capabilities: {:?}", config.runner.capabilities);
    info!("Control plane: {}", config.control_plane.api_url);

    // 共享配置：控制面下发的更新（Docker 配置热更新等）写入此配置，
    // Worker / Executor 在每个任务开始时读取最新快照
    let shared_config = Arc::new(RwLock::new(config));

    // 创建控制面客户端（注册成功后复用于心跳，保持 Docker 配置状态）
    let mut client = ControlPlaneClient::new(shared_config.read().await.clone());

    // 注册 Runner（失败时重试）
    let registration = loop {
        match client.register().await {
            Ok(registration) => {
                info!("Registered successfully with ID: {}", registration.runner_id);
                break registration;
            }
            Err(e) => {
                error!("Registration failed: {}", e);
                warn!("Retrying in 10 seconds...");
                time::sleep(Duration::from_secs(10)).await;
            }
        }
    };

    // 应用控制面下发到共享配置：Docker 配置与 RabbitMQ 配置（以其为准）
    {
        let mut cfg = shared_config.write().await;
        if let Some(docker_cfg) = client.get_docker_config().await {
            info!("Applying Docker configuration from control plane");
            cfg.apply_docker_config(docker_cfg);
        }
        if let Some(rabbitmq) = &registration.rabbitmq {
            if let Some(exchange) = &rabbitmq.exchange
                && !exchange.is_empty()
            {
                info!("Using control-plane exchange: {}", exchange);
                cfg.message_queue.exchange = exchange.clone();
            }
            if let Some(queue_name) = &rabbitmq.queue_name
                && !queue_name.is_empty()
            {
                info!("Using control-plane queue: {}", queue_name);
                cfg.message_queue.queue_name_override = Some(queue_name.clone());
            }
            if let Some(pattern) = &rabbitmq.routing_key_pattern
                && !pattern.is_empty()
            {
                info!("Using control-plane routing key pattern: {}", pattern);
                cfg.message_queue.routing_key_pattern = Some(pattern.clone());
            }
        }
    }

    // 心跳间隔：使用控制面下发的值（已在 client 内钳制到 5..=300 秒）
    let heartbeat_interval = Duration::from_secs(registration.heartbeat_interval_secs);

    // 共享当前作业计数（Worker 在任务开始/到达终态时增减，心跳任务读取上报）
    let current_jobs = Arc::new(AtomicUsize::new(0));

    // 订阅配置更新通知：心跳/注册收到的 Docker 配置变更同步写入共享配置
    let mut config_update_rx = client.config_update_receiver();
    let config_sync_handle = tokio::spawn({
        let shared_config = shared_config.clone();
        async move {
            loop {
                match config_update_rx.changed().await {
                    Ok(_) => {
                        // 先克隆出值，避免 watch::Ref 跨 await 持有（非 Send）
                        let update = config_update_rx.borrow_and_update().clone();
                        if let Some(docker_cfg) = update {
                            info!("Applying Docker configuration update to shared config");
                            shared_config.write().await.apply_docker_config(docker_cfg);
                        }
                    }
                    Err(e) => {
                        // 通道滞后（burst 丢失部分更新）或发送端关闭；
                        // 通道仍可用时继续等待，已关闭则结束同步任务
                        warn!("Config update channel error: {}", e);
                        match config_update_rx.has_changed() {
                            Ok(_) => continue,
                            Err(_) => break,
                        }
                    }
                }
            }
        }
    });

    // 启动心跳任务（复用注册用的 client，保持 Docker 配置与计数器状态）
    let current_jobs_hb = current_jobs.clone();
    let heartbeat_handle = tokio::spawn(async move {
        *client.current_jobs_mut() = current_jobs_hb;
        let mut interval = time::interval(heartbeat_interval);

        loop {
            interval.tick().await;
            match client.send_heartbeat().await {
                Ok(config_updated) => {
                    if config_updated {
                        info!("Docker configuration was updated from heartbeat");
                    }
                }
                Err(e) => {
                    error!("Heartbeat failed: {}", e);
                }
            }
        }
    });

    // 启动任务 Worker
    let shared_config_w = shared_config.clone();
    let current_jobs_w = current_jobs.clone();
    let worker_handle = tokio::spawn(async move {
        loop {
            match TaskWorker::new(shared_config_w.clone(), current_jobs_w.clone()).await {
                Ok(worker) => {
                    info!("Task worker started");

                    if let Err(e) = worker.run().await {
                        error!("Worker error: {}", e);
                    }

                    warn!("Worker exited, restarting in 5 seconds...");
                    time::sleep(Duration::from_secs(5)).await;
                }
                Err(e) => {
                    error!("Failed to create worker: {}", e);
                    warn!("Retrying in 10 seconds...");
                    time::sleep(Duration::from_secs(10)).await;
                }
            }
        }
    });

    info!("ops-runner is running");

    // 等待任务完成（实际上会一直运行）
    tokio::select! {
        _ = heartbeat_handle => {
            error!("Heartbeat task exited unexpectedly");
        }
        _ = config_sync_handle => {
            error!("Config sync task exited unexpectedly");
        }
        _ = worker_handle => {
            error!("Worker task exited unexpectedly");
        }
    }

    Ok(())
}
