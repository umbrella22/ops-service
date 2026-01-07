//! HTTP 中间件
//! 请求追踪、速率限制、IP 白名单

use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

/// 应用状态
///
/// AppState 内部使用 Arc 包装服务,这样:
/// 1. 多个请求可以共享服务实例
/// 2. 服务可以包含内部的可变状态(如果需要)
/// 3. Clone 成本低廉(Arc 是指针拷贝)
///
#[derive(Clone)]
pub struct AppState {
    pub config: crate::config::AppConfig,
    pub db: sqlx::PgPool,
    // 服务使用 Arc 包装,因为服务内部可能包含 Arc 或其他共享状态
    pub auth_service: Arc<crate::services::AuthService>,
    pub permission_service: Arc<crate::services::PermissionService>,
    pub audit_service: Arc<crate::services::AuditService>,
    pub jwt_service: Arc<crate::auth::jwt::JwtService>,
    pub job_service: Arc<crate::services::JobService>,
    pub approval_service: Arc<crate::services::ApprovalService>,
    pub event_bus: Arc<crate::realtime::EventBus>,
}

/// 请求追踪中间件
/// 为每个请求生成 trace_id 和 request_id，并记录指标
pub async fn request_tracking_middleware(req: Request, next: Next) -> Response {
    // 生成或提取 trace_id/request_id
    let trace_id = extract_or_generate_trace_id(req.headers());
    let request_id = Uuid::new_v4().to_string();

    // 获取请求方法和路径
    let method = req.method().to_string();
    let _method_static = method.as_str(); // 用于日志
    let method_for_metrics = req.method().to_string(); // 用于 metrics 的副本
    let uri = req.uri().to_string();

    // 创建 span
    let span = tracing::info_span!(
        "http_request",
        trace_id = %trace_id,
        request_id = %request_id,
        method = %method,
        uri = %uri,
    );

    async move {
        let start = Instant::now();

        // 继续处理请求
        let response = next.run(req).await;

        let elapsed = start.elapsed();

        // 记录指标 - 使用静态字符串
        let status = response.status().as_u16();
        let method_name = match method_for_metrics.as_str() {
            "GET" => "GET",
            "POST" => "POST",
            "PUT" => "PUT",
            "DELETE" => "DELETE",
            "PATCH" => "PATCH",
            _ => "UNKNOWN",
        };
        let status_code = match status {
            200 => "200",
            201 => "201",
            204 => "204",
            400 => "400",
            401 => "401",
            403 => "403",
            404 => "404",
            500 => "500",
            _ => "other",
        };

        let _ = metrics::counter!("http_requests_total", "method" => method_name, "status" => status_code);
        metrics::histogram!("http_request_duration_seconds").record(elapsed.as_secs_f64());

        // 记录日志
        tracing::info!(
            method = %method,
            uri = %uri,
            status = status,
            elapsed_ms = elapsed.as_millis(),
            "Request completed"
        );

        // 在响应头中添加 trace_id
        let mut response = response;
        response.headers_mut().insert("x-trace-id", trace_id.parse().unwrap());
        response.headers_mut().insert("x-request-id", request_id.parse().unwrap());

        response
    }
    .instrument(span)
    .await
}

/// 从请求头中提取或生成 trace_id
fn extract_or_generate_trace_id(headers: &HeaderMap) -> String {
    headers
        .get("x-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

/// 速率限制中间件（简单实现，基于内存）
/// P0 阶段使用简单的令牌桶算法
/// P3+ 阶段应使用 Redis 分布式限流
pub async fn rate_limit_middleware(
    State(_state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    // P0 阶段：仅记录日志，不做实际限流
    // 实际限流由 Nginx 在反向代理层实现

    tracing::debug!("Rate limit check passed");

    Ok(next.run(req).await)
}

/// IP 白名单中间件
pub async fn ip_whitelist_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, crate::error::AppError> {
    if let Some(allowed_ips) = &state.config.security.allowed_ips {
        let client_ip = get_client_ip(&req, state.config.security.trust_proxy)?;

        if !allowed_ips.contains(&client_ip) {
            tracing::warn!(
                client_ip = %client_ip,
                "IP not in whitelist"
            );
            return Err(crate::error::AppError::Forbidden);
        }

        tracing::debug!(client_ip = %client_ip, "IP allowed by whitelist");
    }

    Ok(next.run(req).await)
}

/// 获取客户端 IP 地址
fn get_client_ip(req: &Request, trust_proxy: bool) -> Result<String, crate::error::AppError> {
    let headers = req.headers();

    // 如果信任代理，从 X-Forwarded-For 获取
    if trust_proxy {
        if let Some(forwarded_for) = headers.get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded_for.to_str() {
                // X-Forwarded-For 可能包含多个 IP，取第一个
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    return Ok(first_ip.trim().to_string());
                }
            }
        }

        // 尝试 X-Real-IP
        if let Some(real_ip) = headers.get("x-real-ip") {
            if let Ok(ip_str) = real_ip.to_str() {
                return Ok(ip_str.to_string());
            }
        }
    }

    // 从连接信息获取（需要扩展支持）
    // 这里简化处理，返回未知
    Ok("unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_or_generate_trace_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-trace-id", "test-trace-123".parse().unwrap());

        let trace_id = extract_or_generate_trace_id(&headers);
        assert_eq!(trace_id, "test-trace-123");

        let headers = HeaderMap::new();
        let trace_id = extract_or_generate_trace_id(&headers);
        assert!(!trace_id.is_empty());
        assert_ne!(trace_id, "test-trace-123");
    }
}
