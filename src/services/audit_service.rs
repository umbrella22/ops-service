//! 审计日志服务

use crate::{
    error::AppError,
    models::audit::*,
    repository::audit_repo::AuditRepository,
};
use sqlx::PgPool;
use uuid::Uuid;

pub struct AuditService {
    db: PgPool,
}

impl AuditService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// 记录审计日志条目
    pub async fn log_action(
        &self,
        subject_id: Uuid,
        subject_type: &str,
        subject_name: Option<&str>,
        action: &str,
        resource_type: &str,
        resource_id: Option<Uuid>,
        resource_name: Option<&str>,
        changes: Option<serde_json::Value>,
        changes_summary: Option<&str>,
        source_ip: Option<&str>,
        user_agent: Option<&str>,
        trace_id: Option<&str>,
        result: &str,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let log = AuditLog {
            id: Uuid::new_v4(),
            subject_id,
            subject_type: subject_type.to_string(),
            subject_name: subject_name.map(|s| s.to_string()),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id,
            resource_name: resource_name.map(|s| s.to_string()),
            changes,
            changes_summary: changes_summary.map(|s| s.to_string()),
            source_ip: source_ip.map(|s| s.to_string()),
            user_agent: user_agent.map(|s| s.to_string()),
            trace_id: trace_id.map(|s| s.to_string()),
            request_id: None, // 可以从请求上下文中提取
            result: result.to_string(),
            error_message: error_message.map(|s| s.to_string()),
            occurred_at: chrono::Utc::now(),
        };

        let repo = AuditRepository::new(self.db.clone());
        repo.insert_audit_log(&log).await?;

        Ok(())
    }

    /// 查询审计日志
    pub async fn query_logs(
        &self,
        filters: &AuditLogFilters,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditLog>, AppError> {
        let repo = AuditRepository::new(self.db.clone());
        repo.query_audit_logs(filters, limit, offset).await
    }

    /// 查询审计日志数量
    pub async fn count_logs(&self, filters: &AuditLogFilters) -> Result<i64, AppError> {
        let repo = AuditRepository::new(self.db.clone());
        repo.count_audit_logs(filters).await
    }

    /// 查询登录事件
    pub async fn query_login_events(
        &self,
        user_id: Option<Uuid>,
        event_type: Option<&str>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: i64,
    ) -> Result<Vec<LoginEvent>, AppError> {
        let repo = AuditRepository::new(self.db.clone());
        repo.query_login_events(user_id, event_type, start_time, end_time, limit)
            .await
    }
}
