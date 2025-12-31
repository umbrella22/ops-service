//! 健康检查 API 集成测试

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;
use common::{create_test_app_state, setup_test_db};

#[tokio::test]
async fn test_health_endpoint() {
    // 设置测试环境
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    // 创建应用
    let app = ops_system::routes::create_router(state);

    // 发送请求
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // 验证响应
    assert_eq!(response.status(), StatusCode::OK);

    // 读取响应体
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
    assert!(json["uptime_secs"].is_number());
}

#[tokio::test]
async fn test_readiness_endpoint() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    let app = ops_system::routes::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["ready"], true);
    assert!(json["checks"].is_array());
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    let app = ops_system::routes::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert!(json["process_uptime_secs"].is_number());
    assert!(json["db_pool_size"].is_number());
}

#[tokio::test]
async fn test_not_found_endpoint() {
    let config = common::create_test_config();
    let pool = setup_test_db(&config).await;
    let state = create_test_app_state(pool).await;

    let app = ops_system::routes::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
