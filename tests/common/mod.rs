//! 测试公共模块
//! 提供测试辅助函数和测试工具

use ops_system::{
    auth::jwt::JwtService,
    config::{AppConfig, DatabaseConfig, LoggingConfig, SecurityConfig, ServerConfig},
    db,
    middleware::AppState,
    services::{AuditService, AuthService, PermissionService},
};
use secrecy::Secret;
use sqlx::PgPool;
use std::sync::Arc;

/// 创建测试配置
pub fn create_test_config() -> AppConfig {
    // 从环境变量获取测试数据库 URL，如果没有则使用默认值
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://postgres:postgres@localhost:5432/ops_system_test".to_string()
    });

    AppConfig {
        server: ServerConfig {
            addr: "127.0.0.1:0".to_string(), // 使用随机端口
            graceful_shutdown_timeout_secs: 5,
        },
        database: DatabaseConfig {
            url: Secret::new(database_url),
            max_connections: 5,
            min_connections: 1,
            acquire_timeout_secs: 5,
            idle_timeout_secs: 300,
            max_lifetime_secs: 1800,
        },
        logging: LoggingConfig {
            level: "debug".to_string(),
            format: "pretty".to_string(),
        },
        security: SecurityConfig {
            jwt_secret: Secret::new("test-secret-key-for-testing-only-min-32-chars".to_string()),
            access_token_exp_secs: 300,   // 5分钟用于测试
            refresh_token_exp_secs: 3600, // 1小时用于测试
            password_min_length: 8,
            password_require_uppercase: true,
            password_require_digit: true,
            password_require_special: false,
            max_login_attempts: 5,
            login_lockout_duration_secs: 300,
            rate_limit_rps: 1000,
            trust_proxy: false,
            allowed_ips: None,
        },
    }
}

/// 初始化测试数据库
pub async fn setup_test_db(config: &AppConfig) -> PgPool {
    let pool = db::create_pool(&config.database)
        .await
        .expect("Failed to create test database pool");

    // 运行迁移
    db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    // 清理测试数据（如果有）
    sqlx::query("TRUNCATE TABLE audit_logs, refresh_tokens, assets_hosts, asset_groups, users, roles CASCADE")
        .execute(&pool)
        .await
        .ok(); // 允许失败（表可能还不存在）

    pool
}

/// 创建测试应用状态
pub async fn create_test_app_state(pool: PgPool) -> Arc<AppState> {
    let config = create_test_config();
    let jwt_service =
        Arc::new(JwtService::from_config(&config).expect("Failed to create JWT service"));
    let auth_service =
        Arc::new(AuthService::new(pool.clone(), jwt_service.clone(), Arc::new(config.clone())));
    let permission_service = Arc::new(PermissionService::new(pool.clone()));
    let audit_service = Arc::new(AuditService::new(pool.clone()));

    Arc::new(AppState {
        config,
        db: pool,
        auth_service,
        permission_service,
        audit_service,
        jwt_service,
    })
}

/// 清理测试数据
pub async fn cleanup_test_db(pool: &PgPool) {
    sqlx::query("TRUNCATE TABLE audit_logs, refresh_tokens, assets_hosts, asset_groups, users, roles CASCADE")
        .execute(pool)
        .await
        .expect("Failed to cleanup test database");
}

/// 创建测试用户
pub async fn create_test_user(
    pool: &PgPool,
    username: &str,
    password: &str,
    email: &str,
) -> Result<uuid::Uuid, Box<dyn std::error::Error>> {
    use chrono::Utc;
    use ops_system::auth::password::PasswordHasher;

    let hasher = PasswordHasher::new();
    let password_hash = hasher.hash(password)?;

    let user_id = uuid::Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO users (id, username, password_hash, email, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(username)
    .bind(&password_hash)
    .bind(email)
    .bind(Utc::now())
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(user_id)
}

/// 创建测试角色
pub async fn create_test_role(
    pool: &PgPool,
    name: &str,
    description: &str,
) -> Result<uuid::Uuid, Box<dyn std::error::Error>> {
    use chrono::Utc;

    let role_id = uuid::Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO roles (id, name, description, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(role_id)
    .bind(name)
    .bind(description)
    .bind(Utc::now())
    .bind(Utc::now())
    .execute(pool)
    .await?;

    Ok(role_id)
}

/// 为用户分配角色
pub async fn assign_role_to_user(
    pool: &PgPool,
    user_id: uuid::Uuid,
    role_id: uuid::Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    sqlx::query("INSERT INTO user_roles (user_id, role_id, assigned_at) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(role_id)
        .bind(chrono::Utc::now())
        .execute(pool)
        .await?;

    Ok(())
}

/// 测试用的用户数据
pub struct TestData {
    pub pool: PgPool,
    pub user_id: uuid::Uuid,
    pub username: String,
    pub password: String,
}

/// 设置完整的测试数据
pub async fn setup_test_data(pool: &PgPool) -> TestData {
    let username = "testuser";
    let password = "TestPass123";
    let email = "test@example.com";

    let user_id = create_test_user(pool, username, password, email)
        .await
        .expect("Failed to create test user");

    TestData {
        pool: pool.clone(),
        user_id,
        username: username.to_string(),
        password: password.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_test_config() {
        let config = create_test_config();
        assert_eq!(config.server.addr, "127.0.0.1:0");
        assert_eq!(config.security.access_token_exp_secs, 300);
    }

    #[tokio::test]
    #[ignore] // 需要数据库
    async fn test_setup_test_db() {
        let config = create_test_config();
        let pool = setup_test_db(&config).await;
        assert!(pool.size() > 0);
    }
}
