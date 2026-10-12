//! 权限检查服务

use crate::{error::AppError, models::role::*, repository::role_repo::RoleRepository};
use sqlx::PgPool;
use uuid::Uuid;

pub struct PermissionService {
    db: PgPool,
}

impl PermissionService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    /// 检查用户是否拥有权限
    ///
    /// 单条 SQL 判定（此前对每个角色绑定循环单查 role_has_permission，
    /// N 个绑定即 N+1 次查询）。
    pub async fn check_permission(
        &self,
        user_id: Uuid,
        resource: &str,
        action: &str,
        scope_type: Option<&str>,
        scope_value: Option<&str>,
    ) -> Result<bool, AppError> {
        // scope_type = None 表示"是否拥有该权限"（资源级作用域过滤由
        // filter_resources_by_scope / 各列表接口负责）；
        // 旧实现对 None 只认 global 绑定，导致环境/组作用域角色的所有
        // API 调用一律 403——作用域 RBAC 实际不可用。
        let scope_filter = match (scope_type, scope_value) {
            (Some(st), Some(sv)) => {
                // 要求特定作用域：global 绑定或完全匹配的作用域绑定
                format!(
                    "AND (rb.scope_type = 'global' OR (rb.scope_type = '{}' AND rb.scope_value = '{}'))",
                    st.replace('\'', "''"),
                    sv.replace('\'', "''")
                )
            }
            _ => String::new(),
        };

        let sql = format!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM role_bindings rb
                JOIN role_permissions rp ON rp.role_id = rb.role_id
                JOIN permissions p ON rp.permission_id = p.id
                WHERE rb.user_id = $1
                  AND p.resource = $2
                  AND p.action = $3
                  {}
            )
            "#,
            scope_filter
        );

        let has: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(resource)
            .bind(action)
            .fetch_one(&self.db)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to check permission");
                AppError::database("Failed to check permission")
            })?;

        Ok(has)
    }

    /// 检查权限，如果无权限则返回错误
    pub async fn require_permission(
        &self,
        user_id: Uuid,
        resource: &str,
        action: &str,
        scope_type: Option<&str>,
        scope_value: Option<&str>,
    ) -> Result<(), AppError> {
        let has_permission = self
            .check_permission(user_id, resource, action, scope_type, scope_value)
            .await?;

        if !has_permission {
            tracing::warn!(
                user_id = %user_id,
                resource = %resource,
                action = %action,
                "Permission denied"
            );
            return Err(AppError::Forbidden);
        }

        Ok(())
    }

    /// 检查权限范围是否匹配
    ///
    /// 保留给未来按作用域精确校验的调用方使用；当前所有调用点都以
    /// required_type=None 做权限级判定（作用域过滤在资源层完成）。
    #[allow(dead_code)]
    fn scope_matches(
        &self,
        binding: &RoleBinding,
        required_type: Option<&str>,
        required_value: Option<&str>,
    ) -> bool {
        match binding.scope_type.as_str() {
            "global" => {
                // 全局范围匹配所有
                true
            }
            "group" => {
                // 必须要求 group 范围且值匹配
                if let Some("group") = required_type
                    && let Some(required) = required_value
                {
                    return binding.scope_value.as_ref().is_some_and(|v| v == required);
                }
                false
            }
            "environment" => {
                // 必须要求 environment 范围且值匹配
                if let Some("environment") = required_type
                    && let Some(required) = required_value
                {
                    return binding.scope_value.as_ref().is_some_and(|v| v == required);
                }
                false
            }
            _ => false,
        }
    }

    /// 根据用户的权限范围过滤资源
    pub async fn filter_resources_by_scope(
        &self,
        user_id: Uuid,
        scope_type: &str,
    ) -> Result<Vec<String>, AppError> {
        let role_repo = RoleRepository::new(self.db.clone());
        let bindings = role_repo.get_user_role_bindings(user_id).await?;

        let mut allowed_values = Vec::new();

        for binding in bindings {
            if binding.scope_type == scope_type {
                if let Some(value) = binding.scope_value {
                    allowed_values.push(value);
                }
            } else if binding.scope_type == "global" {
                // 用户拥有全局访问权限
                return Ok(vec!["*".to_string()]);
            }
        }

        Ok(allowed_values)
    }

    /// 检查用户是否是管理员
    pub async fn is_admin(&self, user_id: Uuid) -> Result<bool, AppError> {
        let role_repo = RoleRepository::new(self.db.clone());
        let bindings = role_repo.get_user_role_bindings(user_id).await?;

        for binding in bindings {
            if binding.role_name == "admin" && binding.scope_type == "global" {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 获取用户的所有权限摘要
    pub async fn get_user_permissions(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<PermissionSummary>, AppError> {
        let role_repo = RoleRepository::new(self.db.clone());
        let bindings = role_repo.get_user_role_bindings(user_id).await?;

        let mut permissions = Vec::new();

        for binding in bindings {
            let role_permissions = role_repo.get_role_permissions(binding.role_id).await?;

            for perm in role_permissions {
                // 添加权限范围信息
                permissions.push(PermissionSummary {
                    resource: perm.resource.clone(),
                    action: perm.action.clone(),
                    description: perm.description,
                });
            }
        }

        // 去重
        permissions.sort_by(|a, b| (&a.resource, &a.action).cmp(&(&b.resource, &b.action)));
        permissions.dedup_by(|a, b| a.resource == b.resource && a.action == b.action);

        Ok(permissions)
    }
}
