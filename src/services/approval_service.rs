//! Approval service
//! P3 阶段：审批流服务

use chrono::{Duration, Utc};
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tracing::{error, info, instrument};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::approval::*;
use crate::models::asset::Host;
use crate::models::job::Job;
use crate::realtime::{EventBus, RealtimeEvent};
use crate::services::audit_service::{AuditAction, AuditService};

/// 审批服务
pub struct ApprovalService {
    db: Pool<Postgres>,
    audit_service: Arc<AuditService>,
    event_bus: Arc<EventBus>,
}

impl ApprovalService {
    /// 创建新的审批服务
    pub fn new(
        db: Pool<Postgres>,
        audit_service: Arc<AuditService>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            db,
            audit_service,
            event_bus,
        }
    }

    /// 创建审批请求
    #[instrument(skip(self, request))]
    pub async fn create_approval_request(
        &self,
        request: CreateApprovalRequestRequest,
        requested_by: Uuid,
    ) -> Result<ApprovalRequest> {
        info!(title = %request.title, "Creating approval request");

        // 计算过期时间
        let expires_at = if let Some(timeout_mins) = request.timeout_mins {
            Some(Utc::now() + Duration::minutes(timeout_mins as i64))
        } else {
            None // 默认不超时
        };

        // 创建审批请求
        let approval_id = Uuid::new_v4();
        let approval_request = sqlx::query_as::<_, ApprovalRequest>(
            r#"
            INSERT INTO approval_requests (
                id, job_id, request_type, title, description,
                triggers, required_approvers, approval_group_id,
                status, current_approvals, requested_by, requested_at,
                timeout_mins, expires_at, metadata
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8,
                'pending', 0, $9, NOW(),
                $10, $11, $12
            ) RETURNING *
            "#,
        )
        .bind(approval_id)
        .bind(request.job_id)
        .bind(&request.request_type)
        .bind(&request.title)
        .bind(&request.description)
        .bind(&request.triggers)
        .bind(request.required_approvers)
        .bind(request.approval_group_id)
        .bind(requested_by)
        .bind(request.timeout_mins)
        .bind(expires_at)
        .bind(&request.metadata)
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create approval request");
            AppError::database("Failed to create approval request")
        })?;

        // 记录审计
        self.audit_service
            .log_action_simple(
                requested_by,
                AuditAction::ApprovalCreate,
                Some("approval_requests"),
                Some(approval_id),
                Some(&format!("Approval request: {}", request.title)),
                None,
            )
            .await?;

        // 发布新审批请求事件
        if let Some(job_id) = request.job_id {
            self.event_bus.publish(RealtimeEvent::NewApprovalRequest {
                approval_id,
                job_id: Some(job_id),
                title: request.title.clone(),
                requested_by,
            })?;
        }

        info!(approval_id = %approval_id, "Approval request created successfully");
        Ok(approval_request)
    }

    /// 获取审批请求详情
    #[instrument(skip(self))]
    pub async fn get_approval_request(&self, approval_id: Uuid) -> Result<ApprovalRequest> {
        sqlx::query_as::<_, ApprovalRequest>(
            "SELECT * FROM approval_requests WHERE id = $1"
        )
        .bind(approval_id)
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                AppError::not_found("Approval request not found")
            } else {
                error!(error = %e, approval_id = %approval_id, "Failed to fetch approval request");
                AppError::database("Failed to fetch approval request")
            }
        })
    }

    /// 查询审批请求列表
    #[instrument(skip(self))]
    pub async fn list_approval_requests(
        &self,
        filters: ApprovalListFilters,
    ) -> Result<Vec<ApprovalRequest>> {
        let mut query = String::from("SELECT * FROM approval_requests WHERE 1=1");
        let mut count = 0;

        if filters.status.is_some() {
            count += 1;
            query.push_str(&format!(" AND status = ${}", count));
        }
        if filters.requested_by.is_some() {
            count += 1;
            query.push_str(&format!(" AND requested_by = ${}", count));
        }
        if filters.date_from.is_some() {
            count += 1;
            query.push_str(&format!(" AND requested_at >= ${}", count));
        }
        if filters.date_to.is_some() {
            count += 1;
            query.push_str(&format!(" AND requested_at <= ${}", count));
        }

        query.push_str(" ORDER BY requested_at DESC LIMIT 100");

        let mut q = sqlx::query_as::<_, ApprovalRequest>(&query);

        if let Some(status) = filters.status {
            q = q.bind(status);
        }
        if let Some(requested_by) = filters.requested_by {
            q = q.bind(requested_by);
        }
        if let Some(date_from) = filters.date_from {
            q = q.bind(date_from);
        }
        if let Some(date_to) = filters.date_to {
            q = q.bind(date_to);
        }

        q.fetch_all(&self.db).await.map_err(|e| {
            error!(error = %e, "Failed to fetch approval requests");
            AppError::database("Failed to fetch approval requests")
        })
    }

    /// 审批请求（批准或拒绝）
    #[instrument(skip(self, request))]
    pub async fn approve_request(
        &self,
        approval_id: Uuid,
        approver_id: Uuid,
        approver_name: String,
        request: ApproveRequestRequest,
    ) -> Result<()> {
        info!(
            approval_id = %approval_id,
            approver_id = %approver_id,
            decision = ?request.decision,
            "Processing approval"
        );

        let mut tx = self.db.begin().await.map_err(|e| {
            error!(error = %e, "Failed to begin transaction");
            AppError::database("Failed to begin transaction")
        })?;

        // 获取审批请求
        let approval_req = sqlx::query_as::<_, ApprovalRequest>(
            "SELECT * FROM approval_requests WHERE id = $1 FOR UPDATE",
        )
        .bind(approval_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                AppError::not_found("Approval request not found")
            } else {
                error!(error = %e, "Failed to fetch approval request");
                AppError::database("Failed to fetch approval request")
            }
        })?;

        // 检查审批请求状态
        if !matches!(approval_req.status, ApprovalStatus::Pending) {
            return Err(AppError::validation("Approval request is not pending"));
        }

        // 检查是否已经过期
        if let Some(expires_at) = approval_req.expires_at {
            if Utc::now() > expires_at {
                // 标记为超时
                sqlx::query("UPDATE approval_requests SET status = 'timeout', completed_at = NOW() WHERE id = $1")
                    .bind(approval_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| {
                        error!(error = %e, "Failed to update approval status");
                        AppError::database("Failed to update approval status")
                    })?;

                tx.commit().await.map_err(|e| {
                    error!(error = %e, "Failed to commit transaction");
                    AppError::database("Failed to commit transaction")
                })?;

                return Err(AppError::validation("Approval request has expired"));
            }
        }

        // 检查是否已经审批过
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM approval_records WHERE approval_request_id = $1 AND approver_id = $2"
        )
        .bind(approval_id)
        .bind(approver_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to check existing approval");
            AppError::database("Failed to check existing approval")
        })?;

        if existing > 0 {
            return Err(AppError::validation("Already approved or rejected this request"));
        }

        // 创建审批记录
        sqlx::query(
            r#"
            INSERT INTO approval_records (
                id, approval_request_id, approver_id, approver_name,
                decision, comment, approved_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, NOW()
            )
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(approval_id)
        .bind(approver_id)
        .bind(&approver_name)
        .bind(request.decision.clone())
        .bind(&request.comment)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create approval record");
            AppError::database("Failed to create approval record")
        })?;

        // 更新审批请求数量
        let current_approvals = if matches!(request.decision, ApprovalStatus::Approved) {
            approval_req.current_approvals + 1
        } else {
            approval_req.current_approvals
        };

        // 判断是否完成审批
        let new_status = if matches!(request.decision, ApprovalStatus::Rejected) {
            ApprovalStatus::Rejected
        } else if current_approvals >= approval_req.required_approvers {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        };

        let completed_at =
            if matches!(new_status, ApprovalStatus::Approved | ApprovalStatus::Rejected) {
                Some(Utc::now())
            } else {
                None
            };

        sqlx::query(
            "UPDATE approval_requests SET status = $1, current_approvals = $2, completed_at = $3, updated_at = NOW() WHERE id = $4"
        )
        .bind(&new_status)
        .bind(current_approvals)
        .bind(completed_at)
        .bind(approval_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to update approval request");
            AppError::database("Failed to update approval request")
        })?;

        tx.commit().await.map_err(|e| {
            error!(error = %e, "Failed to commit transaction");
            AppError::database("Failed to commit transaction")
        })?;

        // 记录审计
        let audit_action = match request.decision {
            ApprovalStatus::Approved => AuditAction::ApprovalApprove,
            ApprovalStatus::Rejected => AuditAction::ApprovalReject,
            _ => AuditAction::ApprovalCreate, // fallback
        };

        self.audit_service
            .log_action_simple(
                approver_id,
                audit_action,
                Some("approval_requests"),
                Some(approval_id),
                Some(&format!("Decision: {:?}", request.decision)),
                None,
            )
            .await?;

        // 发布审批状态变更事件
        self.event_bus
            .publish(RealtimeEvent::ApprovalStatusChanged {
                approval_id,
                old_status: format!("{:?}", approval_req.status),
                new_status: format!("{:?}", new_status),
            })?;

        info!(
            approval_id = %approval_id,
            new_status = ?new_status,
            current_approvals = current_approvals,
            "Approval processed successfully"
        );

        Ok(())
    }

    /// 取消审批请求
    #[instrument(skip(self))]
    pub async fn cancel_approval_request(
        &self,
        approval_id: Uuid,
        cancelled_by: Uuid,
    ) -> Result<()> {
        info!(approval_id = %approval_id, "Cancelling approval request");

        // 更新审批请求状态
        let updated = sqlx::query(
            "UPDATE approval_requests SET status = 'cancelled', completed_at = NOW(), updated_at = NOW() WHERE id = $1 AND status = 'pending'"
        )
        .bind(approval_id)
        .execute(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to cancel approval request");
            AppError::database("Failed to cancel approval request")
        })?;

        if updated.rows_affected() == 0 {
            return Err(AppError::validation("Approval request cannot be cancelled"));
        }

        // 记录审计
        self.audit_service
            .log_action_simple(
                cancelled_by,
                AuditAction::ApprovalCancel,
                Some("approval_requests"),
                Some(approval_id),
                Some("Cancelled approval request"),
                None,
            )
            .await?;

        info!(approval_id = %approval_id, "Approval request cancelled successfully");
        Ok(())
    }

    /// 创建审批组
    #[instrument(skip(self, request))]
    pub async fn create_approval_group(
        &self,
        request: CreateApprovalGroupRequest,
        created_by: Uuid,
    ) -> Result<ApprovalGroup> {
        info!(name = %request.name, "Creating approval group");

        let group = sqlx::query_as::<_, ApprovalGroup>(
            r#"
            INSERT INTO approval_groups (
                id, name, description, member_ids, required_approvals,
                scope, priority, is_active, created_by
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, true, $8
            ) RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(&request.name)
        .bind(&request.description)
        .bind(&request.member_ids)
        .bind(request.required_approvals)
        .bind(&request.scope)
        .bind(request.priority.unwrap_or(0))
        .bind(created_by)
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create approval group");
            AppError::database("Failed to create approval group")
        })?;

        // 记录审计
        self.audit_service
            .log_action_simple(
                created_by,
                AuditAction::ApprovalGroupCreate,
                Some("approval_groups"),
                Some(group.id),
                Some(&format!("Approval group: {}", request.name)),
                None,
            )
            .await?;

        info!(group_id = %group.id, "Approval group created successfully");
        Ok(group)
    }

    /// 检查作业是否需要审批
    #[instrument(skip(self))]
    pub async fn check_job_requires_approval(
        &self,
        job: &Job,
        target_hosts: &[Host],
    ) -> Result<bool> {
        // 检查是否为生产环境
        let is_production = target_hosts
            .iter()
            .any(|h| h.environment.to_lowercase() == "production");

        // 检查目标主机数量
        let target_count = target_hosts.len() as i32;
        let exceeds_threshold = target_count > 10; // 可配置的阈值

        // 检查命令风险等级
        let command = job.command.as_deref().unwrap_or("");
        let is_high_risk = self.is_high_risk_command(command);

        // 检查是否为关键分组
        let is_critical_group = target_hosts.iter().any(|h| {
            // 检查主机的分组是否被标记为关键
            self.is_critical_group(h.group_id)
        });

        let requires_approval =
            is_production || exceeds_threshold || is_high_risk || is_critical_group;

        if requires_approval {
            info!(
                job_id = %job.id,
                is_production,
                exceeds_threshold,
                is_high_risk,
                is_critical_group,
                "Job requires approval"
            );
        }

        Ok(requires_approval)
    }

    /// 判断是否为高风险命令
    fn is_high_risk_command(&self, command: &str) -> bool {
        let high_risk_patterns = vec![
            "rm -rf",
            "dd if",
            "mkfs",
            ":(){ :|:& };:", // fork bomb
            "format",
            "del /q",
            "shutdown",
            "reboot",
            "> /dev/",
            "truncate -s 0",
        ];

        let command_lower = command.to_lowercase();
        high_risk_patterns
            .iter()
            .any(|pattern| command_lower.contains(&pattern.to_lowercase()))
    }

    /// 检查分组是否为关键分组
    ///
    /// 关键分组的作业需要审批
    /// 检查条件：
    /// 1. 分组标记为 is_critical = true
    /// 2. 分组名称包含 "critical" 或 "prod"
    fn is_critical_group(&self, _group_id: uuid::Uuid) -> bool {
        // TODO: 实现数据库查询来检查分组是否为关键分组
        // 当前使用简化逻辑：检查分组名称

        // 在实际实现中，应该查询数据库：
        // sqlx::query_scalar::<_, bool>(
        //     "SELECT EXISTS(
        //         SELECT 1 FROM asset_groups
        //         WHERE id = $1 AND (is_critical = true OR name ILIKE '%critical%' OR name ILIKE '%prod%')
        //     )"
        // )
        // .bind(group_id)
        // .fetch_one(&self.db)
        // .await
        // .unwrap_or(false)

        // 临时返回 false，等待数据库迁移添加 is_critical 字段
        false
    }

    /// 获取审批统计
    #[instrument(skip(self))]
    pub async fn get_approval_statistics(&self) -> Result<ApprovalStatistics> {
        let total_requests = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM approval_requests")
            .fetch_one(&self.db)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to count total requests");
                AppError::database("Failed to count requests")
            })?;

        let pending_requests = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM approval_requests WHERE status = 'pending'",
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to count pending requests");
            AppError::database("Failed to count requests")
        })?;

        let approved_requests = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM approval_requests WHERE status = 'approved'",
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to count approved requests");
            AppError::database("Failed to count requests")
        })?;

        let rejected_requests = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM approval_requests WHERE status = 'rejected'",
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to count rejected requests");
            AppError::database("Failed to count requests")
        })?;

        let timeout_requests = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM approval_requests WHERE status = 'timeout'",
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to count timeout requests");
            AppError::database("Failed to count requests")
        })?;

        // 计算平均审批时间（仅统计已完成的）
        let avg_time = sqlx::query_scalar::<_, Option<f64>>(
            r#"
            SELECT AVG(EXTRACT(EPOCH FROM (completed_at - requested_at)) / 60)
            FROM approval_requests
            WHERE completed_at IS NOT NULL
            "#,
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to calculate average approval time");
            AppError::database("Failed to calculate statistics")
        })?;

        Ok(ApprovalStatistics {
            total_requests,
            pending_requests,
            approved_requests,
            rejected_requests,
            timeout_requests,
            avg_approval_time_mins: avg_time,
        })
    }
}
