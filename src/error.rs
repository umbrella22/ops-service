//! 统一错误模型
//! 定义所有错误类型和错误响应格式

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;
use thiserror::Error;

/// 应用错误类型
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Authentication failed")]
    Unauthorized,

    #[error("Access denied")]
    Forbidden,

    #[error("Resource not found")]
    NotFound,

    #[error("Invalid request: {0}")]
    BadRequest(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Internal server error")]
    Internal,
}

impl AppError {
    /// 获取 HTTP 状态码
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
            AppError::Database(_) | AppError::Config(_) | AppError::Internal => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// 获取用户友好的错误消息（不包含敏感信息）
    pub fn user_message(&self) -> String {
        match self {
            AppError::Unauthorized => "Authentication failed".to_string(),
            AppError::Forbidden => "Access denied".to_string(),
            AppError::NotFound => "Resource not found".to_string(),
            AppError::BadRequest(msg) => msg.clone(),
            AppError::RateLimitExceeded => "Rate limit exceeded".to_string(),
            AppError::Database(_) => "Database error occurred".to_string(),
            AppError::Config(_) => "Configuration error".to_string(),
            AppError::Internal => "Internal server error".to_string(),
        }
    }

    /// 获取错误码
    pub fn code(&self) -> u16 {
        self.status_code().as_u16()
    }
}

/// 错误响应 DTO
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Serialize)]
pub struct ErrorDetail {
    pub code: u16,
    pub message: String,
    pub request_id: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let request_id = uuid::Uuid::new_v4().to_string();

        let error_response = ErrorResponse {
            error: ErrorDetail {
                code: self.code(),
                message: self.user_message(),
                request_id,
            },
        };

        // 记录错误日志
        tracing::error!(
            code = self.code(),
            message = %self,
            request_id = %error_response.error.request_id,
            "Application error"
        );

        (status, Json(error_response)).into_response()
    }
}

/// 从 String 转换为 AppError::Config
impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Config(s)
    }
}

/// 从 config::ConfigError 转换
impl From<config::ConfigError> for AppError {
    fn from(e: config::ConfigError) -> Self {
        AppError::Config(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(AppError::Unauthorized.code(), 401);
        assert_eq!(AppError::Forbidden.code(), 403);
        assert_eq!(AppError::NotFound.code(), 404);
        assert_eq!(AppError::BadRequest("test".to_string()).code(), 400);
        assert_eq!(AppError::RateLimitExceeded.code(), 429);
    }

    #[test]
    fn test_user_message_no_sensitive_info() {
        let error = AppError::Database(sqlx::Error::RowNotFound);
        let message = error.user_message();
        assert_eq!(message, "Database error occurred");
        assert!(!message.contains("sqlx"));
    }
}
