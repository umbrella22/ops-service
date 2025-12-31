//! 指标处理器
//! 提供 /metrics 端点

use axum::{extract::State, Json};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::middleware::AppState;

/// 指标响应
#[derive(Serialize)]
pub struct MetricsResponse {
    pub http_requests_total: u64,
    pub http_requests_by_status: HashMap<String, u64>,
    pub http_request_duration_p50_ms: f64,
    pub http_request_duration_p95_ms: f64,
    pub http_request_duration_p99_ms: f64,
    pub db_pool_size: u32,
    pub db_pool_idle: u32,
    pub process_uptime_secs: u64,
}

/// 指标暴露端点
pub async fn metrics_export(
    State(state): State<Arc<AppState>>,
) -> Json<MetricsResponse> {
    // 简化实现：返回基础指标
    // 实际生产环境应使用 Prometheus exporter

    Json(MetricsResponse {
        http_requests_total: 0, // 需要从 metrics crate 获取
        http_requests_by_status: HashMap::new(),
        http_request_duration_p50_ms: 0.0,
        http_request_duration_p95_ms: 0.0,
        http_request_duration_p99_ms: 0.0,
        db_pool_size: state.db.size() as u32,
        db_pool_idle: state.db.num_idle() as u32,
        process_uptime_secs: crate::handlers::health::get_uptime(),
    })
}
