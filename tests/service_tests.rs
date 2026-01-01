//! 服务层单元测试

use ops_system::{
    auth::jwt::JwtService,
    config::AppConfig,
    models::auth::*,
    services::{AuditService, AuthService, PermissionService},
};
use secrecy::Secret;

mod common;
use common::{assign_role_to_user, create_test_config, create_test_role, create_test_user};

#[tokio::test]
async fn test_auth_service_login_success() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    // 创建测试用户
    let username = "testuser";
    let password = "TestPass123";
    create_test_user(&pool, username, password, "test@example.com")
        .await
        .expect("Failed to create test user");

    let jwt_service = std::sync::Arc::new(JwtService::from_config(&config).unwrap());
    let auth_service = AuthService::new(pool.clone(), jwt_service, std::sync::Arc::new(config));

    // 执行登录
    let login_req = LoginRequest {
        username: username.to_string(),
        password: password.to_string(),
    };

    let result = auth_service
        .login(login_req, "127.0.0.1", Some("test-agent"))
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(!response.access_token.is_empty());
    assert!(!response.refresh_token.is_empty());
    assert_eq!(response.user.username, username);
}

#[tokio::test]
async fn test_auth_service_login_wrong_password() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let username = "testuser";
    create_test_user(&pool, username, "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let jwt_service = std::sync::Arc::new(JwtService::from_config(&config).unwrap());
    let auth_service = AuthService::new(pool.clone(), jwt_service, std::sync::Arc::new(config));

    let login_req = LoginRequest {
        username: username.to_string(),
        password: "WrongPassword".to_string(),
    };

    let result = auth_service
        .login(login_req, "127.0.0.1", Some("test-agent"))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_auth_service_login_rate_limit() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let username = "testuser";
    create_test_user(&pool, username, "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let jwt_service = std::sync::Arc::new(JwtService::from_config(&config).unwrap());
    let auth_service = AuthService::new(pool.clone(), jwt_service, std::sync::Arc::new(config));

    let login_req = LoginRequest {
        username: username.to_string(),
        password: "WrongPassword".to_string(),
    };

    // 尝试多次错误登录
    for _ in 0..6 {
        let _ = auth_service
            .login(login_req.clone(), "127.0.0.1", Some("test-agent"))
            .await;
    }

    // 现在应该被锁定
    let result = auth_service
        .login(login_req, "127.0.0.1", Some("test-agent"))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_auth_service_refresh_token() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let username = "testuser";
    let password = "TestPass123";
    create_test_user(&pool, username, password, "test@example.com")
        .await
        .expect("Failed to create test user");

    let jwt_service = std::sync::Arc::new(JwtService::from_config(&config).unwrap());
    let auth_service =
        AuthService::new(pool.clone(), jwt_service.clone(), std::sync::Arc::new(config.clone()));

    // 先登录
    let login_req = LoginRequest {
        username: username.to_string(),
        password: password.to_string(),
    };

    let login_response = auth_service
        .login(login_req, "127.0.0.1", Some("test-agent"))
        .await
        .unwrap();

    // 使用 refresh token 获取新的 token pair
    let refresh_req = RefreshTokenRequest {
        refresh_token: login_response.refresh_token.clone(),
    };

    let result = auth_service.refresh_token(refresh_req, "127.0.0.1").await;

    assert!(result.is_ok());
    let new_tokens = result.unwrap();
    assert!(!new_tokens.access_token.is_empty());
    assert!(!new_tokens.refresh_token.is_empty());
}

#[tokio::test]
async fn test_permission_service_check_permission() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    // 创建用户和角色
    let user_id = create_test_user(&pool, "testuser", "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let role_id = create_test_role(&pool, "admin", "Administrator")
        .await
        .expect("Failed to create test role");

    assign_role_to_user(&pool, user_id, role_id)
        .await
        .expect("Failed to assign role");

    let permission_service = PermissionService::new(pool);

    // 检查权限
    let result = permission_service
        .check_permission(user_id, "asset", "read", None)
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_audit_service_log_event() {
    let config = create_test_config();
    let pool = common::setup_test_db(&config).await;

    let user_id = create_test_user(&pool, "testuser", "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let audit_service = AuditService::new(pool);

    // 记录审计事件
    let result = audit_service
        .log_event(
            user_id,
            "user",
            Some(user_id.to_string()),
            "login",
            "success",
            None,
            Some("test-trace-id"),
            Some("127.0.0.1"),
            Some("test-agent"),
        )
        .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_jwt_service_generate_and_validate() {
    let config = create_test_config();
    let jwt_service = JwtService::from_config(&config).unwrap();

    let user_id = uuid::Uuid::new_v4();
    let username = "testuser";
    let roles = vec!["admin".to_string()];
    let scopes = vec!["read".to_string(), "write".to_string()];

    // 生成 token
    let token_pair = jwt_service
        .generate_token_pair(&user_id, username, roles.clone(), scopes)
        .unwrap();

    assert!(!token_pair.access_token.is_empty());
    assert!(!token_pair.refresh_token.is_empty());
    assert!(token_pair.expires_in > 0);

    // 验证 access token
    let claims = jwt_service
        .validate_access_token(&token_pair.access_token)
        .unwrap();

    assert_eq!(claims.sub, user_id.to_string());
    assert_eq!(claims.username, username);
}

#[tokio::test]
async fn test_jwt_service_validate_invalid_token() {
    let config = create_test_config();
    let jwt_service = JwtService::from_config(&config).unwrap();

    let result = jwt_service.validate_access_token("invalid_token");

    assert!(result.is_err());
}
