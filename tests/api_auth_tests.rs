//! 认证 API 集成测试

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

mod common;
use common::{create_test_app_state, create_test_user, setup_test_db};

#[tokio::test]
async fn test_login_success() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;

    // 创建测试用户
    let username = "testuser";
    let password = "TestPass123";
    create_test_user(&pool, username, password, "test@example.com")
        .await
        .expect("Failed to create test user");

    let state = create_test_app_state(pool).await;
    let app = ops_system::routes::create_router(state);

    // 发送登录请求
    let request_body = json!({
        "username": username,
        "password": password
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert!(json["access_token"].is_string());
    assert!(json["refresh_token"].is_string());
    assert!(json["expires_in"].is_number());
    assert_eq!(json["user"]["username"], username);
}

#[tokio::test]
async fn test_login_wrong_password() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;

    let username = "testuser";
    create_test_user(&pool, username, "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    let state = create_test_app_state(pool).await;
    let app = ops_system::routes::create_router(state);

    let request_body = json!({
        "username": username,
        "password": "WrongPassword"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_user_not_found() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    let app = ops_system::routes::create_router(state);

    let request_body = json!({
        "username": "nonexistent",
        "password": "TestPass123"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_current_user() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;

    let username = "testuser";
    let password = "TestPass123";
    create_test_user(&pool, username, password, "test@example.com")
        .await
        .expect("Failed to create test user");

    let state = create_test_app_state(pool).await;
    let app = ops_system::routes::create_router(state);

    // 先登录获取 token
    let login_body = json!({
        "username": username,
        "password": password
    });

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let login_body_bytes = login_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let login_json: serde_json::Value = serde_json::from_slice(&login_body_bytes).unwrap();
    let access_token = login_json["access_token"].as_str().unwrap();

    // 使用 token 获取当前用户信息
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/auth/me")
                .header(header::AUTHORIZATION, format!("Bearer {}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["username"], username);
}

#[tokio::test]
async fn test_get_current_user_without_token() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    let app = ops_system::routes::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_logout() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;

    let username = "testuser";
    let password = "TestPass123";
    create_test_user(&pool, username, password, "test@example.com")
        .await
        .expect("Failed to create test user");

    let state = create_test_app_state(pool).await;
    let app = ops_system::routes::create_router(state);

    // 先登录
    let login_body = json!({
        "username": username,
        "password": password
    });

    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let login_body_bytes = login_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let login_json: serde_json::Value = serde_json::from_slice(&login_body_bytes).unwrap();
    let refresh_token = login_json["refresh_token"].as_str().unwrap();

    // 登出
    let logout_body = json!({
        "refresh_token": refresh_token
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/logout")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(logout_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
