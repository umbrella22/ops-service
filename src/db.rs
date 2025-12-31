//! 数据库连接池与迁移管理
//! 提供 PostgreSQL 连接池、迁移执行和健康检查

use crate::config::DatabaseConfig;
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

/// 创建数据库连接池
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, DbError> {
    let db_url = config.url.expose_secret();

    tracing::debug!("Creating database connection pool...");

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .max_lifetime(Duration::from_secs(config.max_lifetime_secs))
        .test_before_acquire(true)
        .connect(db_url)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create database pool: {}", e);
            DbError::ConnectionFailed(e.to_string())
        })?;

    tracing::info!(
        max_connections = config.max_connections,
        min_connections = config.min_connections,
        "Database pool created successfully"
    );

    Ok(pool)
}

/// 运行数据库迁移
pub async fn run_migrations(pool: &PgPool) -> Result<(), DbError> {
    tracing::info!("Running database migrations...");

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| {
            tracing::error!("Migration failed: {}", e);
            DbError::MigrationFailed(e.to_string())
        })?;

    tracing::info!("Migrations completed successfully");
    Ok(())
}

/// 数据库健康检查
pub async fn health_check(pool: &PgPool) -> HealthStatus {
    match sqlx::query("SELECT 1").fetch_one(pool).await {
        Ok(_) => {
            tracing::debug!("Database health check: OK");
            HealthStatus::Healthy
        }
        Err(e) => {
            tracing::warn!("Database health check failed: {}", e);
            HealthStatus::Unhealthy(e.to_string())
        }
    }
}

/// 记录数据库连接池指标
pub fn record_pool_metrics(pool: &PgPool) {
    metrics::gauge!("db.pool.size").set(pool.size() as f64);
    metrics::gauge!("db.pool.idle").set(pool.num_idle() as f64);
}

/// 数据库错误类型
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Migration failed: {0}")]
    MigrationFailed(String),

    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),
}

/// 健康状态
#[derive(Debug, Clone)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status() {
        let healthy = HealthStatus::Healthy;
        let unhealthy = HealthStatus::Unhealthy("Connection refused".to_string());

        match healthy {
            HealthStatus::Healthy => assert!(true),
            _ => assert!(false),
        }

        match unhealthy {
            HealthStatus::Unhealthy(msg) => assert_eq!(msg, "Connection refused"),
            _ => assert!(false),
        }
    }
}
