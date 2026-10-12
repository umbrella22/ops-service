//! Asset repository (资产数据访问)

use crate::{error::AppError, models::asset::*};
use sqlx::{AssertSqlSafe, PgPool, Row};
use uuid::Uuid;

pub struct AssetRepository {
    db: PgPool,
}

impl AssetRepository {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    // ==================== Groups ====================

    /// 创建资产组
    pub async fn create_group(
        &self,
        req: &CreateGroupRequest,
        created_by: Uuid,
    ) -> Result<AssetGroup, AppError> {
        let group = sqlx::query_as::<_, AssetGroup>(
            r#"
            INSERT INTO assets_groups (name, description, environment, parent_id, created_by)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(&req.name)
        .bind(&req.description)
        .bind(&req.environment)
        .bind(req.parent_id)
        .bind(created_by)
        .fetch_one(&self.db)
        .await?;

        Ok(group)
    }

    /// 获取资产组
    pub async fn get_group(&self, id: Uuid) -> Result<Option<AssetGroup>, AppError> {
        let group = sqlx::query_as::<_, AssetGroup>("SELECT * FROM assets_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.db)
            .await?;

        Ok(group)
    }

    /// 列出资产组
    pub async fn list_groups(
        &self,
        environment: Option<&str>,
    ) -> Result<Vec<AssetGroup>, AppError> {
        let groups = if let Some(env) = environment {
            sqlx::query_as::<_, AssetGroup>(
                "SELECT * FROM assets_groups WHERE environment = $1 ORDER BY name",
            )
            .bind(env)
            .fetch_all(&self.db)
            .await?
        } else {
            sqlx::query_as::<_, AssetGroup>(
                "SELECT * FROM assets_groups ORDER BY environment, name",
            )
            .fetch_all(&self.db)
            .await?
        };

        Ok(groups)
    }

    /// 更新资产组
    pub async fn update_group(
        &self,
        id: Uuid,
        req: &UpdateGroupRequest,
    ) -> Result<Option<AssetGroup>, AppError> {
        let group = sqlx::query_as::<_, AssetGroup>(
            r#"
            UPDATE assets_groups
            SET
                name = COALESCE($2, name),
                description = COALESCE($3, description),
                environment = COALESCE($4, environment),
                parent_id = COALESCE($5, parent_id),
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(&req.name)
        .bind(&req.description)
        .bind(&req.environment)
        .bind(req.parent_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(group)
    }

    /// 删除资产组
    pub async fn delete_group(&self, id: Uuid) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM assets_groups WHERE id = $1")
            .bind(id)
            .execute(&self.db)
            .await
            .map_err(|e| {
                // 组下仍有主机时 FK RESTRICT：返回业务化 400 而非裸 500
                if let sqlx::Error::Database(ref db_err) = e
                    && db_err.code().as_deref() == Some("23503")
                {
                    return AppError::validation(
                        "Asset group still contains hosts; move or delete them first",
                    );
                }
                AppError::Database(e)
            })?;

        Ok(result.rows_affected() > 0)
    }

    // ==================== Hosts ====================

    /// 创建主机
    pub async fn create_host(
        &self,
        req: &CreateHostRequest,
        created_by: Uuid,
    ) -> Result<Host, AppError> {
        // SSH 凭据落盘加密（未配置密钥时退化为明文，启动时有警告）
        let ssh_username = encrypt_credential(&req.ssh_username).await?;
        let ssh_password = encrypt_credential(&req.ssh_password).await?;
        let ssh_private_key = encrypt_credential(&req.ssh_private_key).await?;
        let ssh_key_passphrase = encrypt_credential(&req.ssh_key_passphrase).await?;

        let host = sqlx::query_as::<_, Host>(
            r#"
            INSERT INTO assets_hosts (
                identifier, display_name, address, port, group_id, environment,
                tags, owner_id, status, notes, os_type, os_version, created_by,
                ssh_username, ssh_password, ssh_private_key, ssh_key_passphrase,
                host_key_verification, known_hosts
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                    $14, $15, $16, $17, $18, $19)
            RETURNING *
            "#
        )
        .bind(&req.identifier)
        .bind(&req.display_name)
        .bind(&req.address)
        .bind(req.port)
        .bind(req.group_id)
        .bind(&req.environment)
        .bind(sqlx::types::Json(req.tags.clone())) // 转换为 Json
        .bind(req.owner_id)
        .bind(&req.status)
        .bind(&req.notes)
        .bind(&req.os_type)
        .bind(&req.os_version)
        .bind(created_by)
        .bind(&ssh_username)
        .bind(&ssh_password)
        .bind(&ssh_private_key)
        .bind(&ssh_key_passphrase)
        .bind(&req.host_key_verification)
        .bind(req.known_hosts.as_ref().map(|m| sqlx::types::Json(m.clone())))
        .fetch_one(&self.db)
        .await?;

        Ok(host)
    }

    /// 获取主机
    pub async fn get_host(&self, id: Uuid) -> Result<Option<Host>, AppError> {
        let host = sqlx::query_as::<_, Host>("SELECT * FROM assets_hosts WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.db)
            .await?;

        match host {
            Some(h) => Ok(Some(decrypt_host_credentials(h).await?)),
            None => Ok(None),
        }
    }

    /// 根据标识符获取主机
    pub async fn get_host_by_identifier(&self, identifier: &str) -> Result<Option<Host>, AppError> {
        let host = sqlx::query_as::<_, Host>("SELECT * FROM assets_hosts WHERE identifier = $1")
            .bind(identifier)
            .fetch_optional(&self.db)
            .await?;

        match host {
            Some(h) => Ok(Some(decrypt_host_credentials(h).await?)),
            None => Ok(None),
        }
    }

    /// 列出主机
    pub async fn list_hosts(
        &self,
        filters: &HostListFilters,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Host>, AppError> {
        let mut query = String::from("SELECT * FROM assets_hosts WHERE 1=1");
        let mut index = 0;

        if filters.group_id.is_some() {
            index += 1;
            query.push_str(&format!(" AND group_id = ${}", index));
        }
        if let Some(_env) = &filters.environment {
            index += 1;
            query.push_str(&format!(" AND environment = ${}", index));
        }
        if let Some(_status) = &filters.status {
            index += 1;
            query.push_str(&format!(" AND status = ${}", index));
        }
        if let Some(_search) = &filters.search {
            index += 1;
            query.push_str(&format!(
                " AND (identifier ILIKE ${} ESCAPE '\\\\' OR display_name ILIKE ${} ESCAPE '\\\\')",
                index,
                index + 1
            ));
            index += 1;
        }
        if let Some(_tags) = &filters.tags {
            index += 1;
            query.push_str(&format!(" AND tags @> ${}", index));
        }
        // 作用域过滤进 SQL：旧实现先分页再内存过滤，total 恒等于页大小、
        // 跨页丢/重记录
        if filters.allowed_environments.is_some() {
            index += 1;
            query.push_str(&format!(" AND environment = ANY(${})", index));
        }
        if filters.allowed_groups.is_some() {
            index += 1;
            query.push_str(&format!(" AND group_id = ANY(${})", index));
        }

        query.push_str(&format!(" ORDER BY identifier LIMIT ${} OFFSET ${}", index + 1, index + 2));

        let mut query_builder = sqlx::query_as::<_, Host>(AssertSqlSafe(query));

        if let Some(group_id) = &filters.group_id {
            query_builder = query_builder.bind(group_id);
        }
        if let Some(env) = &filters.environment {
            query_builder = query_builder.bind(env);
        }
        if let Some(status) = &filters.status {
            query_builder = query_builder.bind(status);
        }
        let search_pattern;
        if let Some(search) = &filters.search {
            // 转义 %/_/\，防止用户输入注入通配符（%%% 全表扫描 / 枚举）
            search_pattern = format!("%{}%", super::escape_like(search));
            query_builder = query_builder.bind(&search_pattern);
            query_builder = query_builder.bind(&search_pattern);
        }
        if let Some(tags) = &filters.tags {
            query_builder = query_builder.bind(sqlx::types::Json(tags.clone()));
        }
        if let Some(envs) = &filters.allowed_environments {
            query_builder = query_builder.bind(envs.clone());
        }
        if let Some(groups) = &filters.allowed_groups {
            query_builder = query_builder.bind(groups.clone());
        }

        let hosts = query_builder
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db)
            .await?;

        let mut decrypted = Vec::with_capacity(hosts.len());
        for h in hosts {
            decrypted.push(decrypt_host_credentials(h).await?);
        }
        Ok(decrypted)
    }

    /// 更新主机
    ///
    /// 乐观锁：旧实现"先读 version 比较再无条件 UPDATE"，两个并发更新
    /// 都能通过校验并互相覆盖（丢失更新）。现在把 version 作为 UPDATE 的
    /// WHERE 守卫，冲突时更新 0 行并返回 409 语义错误。
    /// 文本字段传空字符串表示清空（COALESCE + NULLIF），传 None 表示不修改。
    pub async fn update_host(
        &self,
        id: Uuid,
        req: &UpdateHostRequest,
        updated_by: Uuid,
    ) -> Result<Option<Host>, AppError> {
        // 转换tags为Json类型
        let tags_json = req.tags.as_ref().map(|t| sqlx::types::Json(t.clone()));

        // 凭据：Some(空串) = 清除，Some(内容) = 加密后更新，None = 不修改
        let ssh_username = encrypt_credential(&req.ssh_username).await?;
        let ssh_password = encrypt_credential(&req.ssh_password).await?;
        let ssh_private_key = encrypt_credential(&req.ssh_private_key).await?;
        let ssh_key_passphrase = encrypt_credential(&req.ssh_key_passphrase).await?;

        let host = sqlx::query_as::<_, Host>(
            r#"
            UPDATE assets_hosts
            SET
                display_name = COALESCE(NULLIF($2, ''), display_name),
                address = COALESCE($3, address),
                port = COALESCE($4, port),
                group_id = COALESCE($5, group_id),
                environment = COALESCE($6, environment),
                tags = COALESCE($7, tags),
                owner_id = COALESCE($8, owner_id),
                status = COALESCE($9, status),
                notes = COALESCE(NULLIF($10, ''), notes),
                os_type = COALESCE(NULLIF($11, ''), os_type),
                os_version = COALESCE(NULLIF($12, ''), os_version),
                updated_by = $13,
                updated_at = NOW(),
                ssh_username = CASE WHEN $14 IS NULL THEN ssh_username ELSE $14 END,
                ssh_password = CASE WHEN $15 IS NULL THEN ssh_password ELSE $15 END,
                ssh_private_key = CASE WHEN $16 IS NULL THEN ssh_private_key ELSE $16 END,
                ssh_key_passphrase = CASE WHEN $17 IS NULL THEN ssh_key_passphrase ELSE $17 END,
                host_key_verification = COALESCE($18, host_key_verification)
            WHERE id = $1 AND version = $19
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(&req.display_name)
        .bind(&req.address)
        .bind(req.port)
        .bind(req.group_id)
        .bind(&req.environment)
        .bind(&tags_json)
        .bind(req.owner_id)
        .bind(&req.status)
        .bind(&req.notes)
        .bind(&req.os_type)
        .bind(&req.os_version)
        .bind(updated_by)
        .bind(&ssh_username)
        .bind(&ssh_password)
        .bind(&ssh_private_key)
        .bind(&ssh_key_passphrase)
        .bind(&req.host_key_verification)
        .bind(req.version)
        .fetch_optional(&self.db)
        .await?;

        if host.is_none() {
            // 区分"不存在"与"版本冲突"
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM assets_hosts WHERE id = $1)",
            )
            .bind(id)
            .fetch_one(&self.db)
            .await
            .unwrap_or(false);
            if !exists {
                return Err(AppError::not_found("Resource not found"));
            }
            return Err(AppError::BadRequest("资源已被其他用户修改，请刷新后重试".to_string()));
        }

        match host {
            Some(h) => Ok(Some(decrypt_host_credentials(h).await?)),
            None => Ok(None),
        }
    }

    /// 删除主机
    pub async fn delete_host(&self, id: Uuid) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM assets_hosts WHERE id = $1")
            .bind(id)
            .execute(&self.db)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// 统计主机数量
    pub async fn count_hosts(&self, filters: &HostListFilters) -> Result<i64, AppError> {
        let mut query = String::from("SELECT COUNT(*) FROM assets_hosts WHERE 1=1");
        let mut index = 0;

        if filters.group_id.is_some() {
            index += 1;
            query.push_str(&format!(" AND group_id = ${}", index));
        }
        if let Some(_env) = &filters.environment {
            index += 1;
            query.push_str(&format!(" AND environment = ${}", index));
        }
        if let Some(_status) = &filters.status {
            index += 1;
            query.push_str(&format!(" AND status = ${}", index));
        }
        if let Some(_search) = &filters.search {
            index += 1;
            query.push_str(&format!(
                " AND (identifier ILIKE ${} ESCAPE '\\\\' OR display_name ILIKE ${} ESCAPE '\\\\')",
                index,
                index + 1
            ));
            index += 1;
        }
        if let Some(_tags) = &filters.tags {
            index += 1;
            query.push_str(&format!(" AND tags @> ${}", index));
        }
        if filters.allowed_environments.is_some() {
            index += 1;
            query.push_str(&format!(" AND environment = ANY(${})", index));
        }
        if filters.allowed_groups.is_some() {
            index += 1;
            query.push_str(&format!(" AND group_id = ANY(${})", index));
        }

        let mut query_builder = sqlx::query(AssertSqlSafe(query));

        if let Some(group_id) = &filters.group_id {
            query_builder = query_builder.bind(group_id);
        }
        if let Some(env) = &filters.environment {
            query_builder = query_builder.bind(env);
        }
        if let Some(status) = &filters.status {
            query_builder = query_builder.bind(status);
        }
        let search_pattern;
        if let Some(search) = &filters.search {
            search_pattern = format!("%{}%", super::escape_like(search));
            query_builder = query_builder.bind(&search_pattern);
            query_builder = query_builder.bind(&search_pattern);
        }
        if let Some(tags) = &filters.tags {
            query_builder = query_builder.bind(sqlx::types::Json(tags.clone()));
        }
        if let Some(envs) = &filters.allowed_environments {
            query_builder = query_builder.bind(envs.clone());
        }
        if let Some(groups) = &filters.allowed_groups {
            query_builder = query_builder.bind(groups.clone());
        }

        let count: i64 = query_builder.fetch_one(&self.db).await?.get(0);
        Ok(count)
    }
}

/// 加密凭据后入库。
/// None 表示不修改；Some(空串) 表示清除（存空串）；Some(内容) 加密存储。
async fn encrypt_credential(value: &Option<String>) -> Result<Option<String>, AppError> {
    match value {
        Some(s) if !s.is_empty() => {
            Ok(Some(crate::services::crypto_service::encrypt(s).await?))
        }
        Some(_) => Ok(Some(String::new())),
        None => Ok(None),
    }
}

/// 解密主机上的 SSH 凭据字段（兼容历史明文数据）。
async fn decrypt_host_credentials(mut host: Host) -> Result<Host, AppError> {
    host.ssh_username = decrypt_field(host.ssh_username).await?;
    host.ssh_password = decrypt_field(host.ssh_password).await?;
    host.ssh_private_key = decrypt_field(host.ssh_private_key).await?;
    host.ssh_key_passphrase = decrypt_field(host.ssh_key_passphrase).await?;
    Ok(host)
}

async fn decrypt_field(value: Option<String>) -> Result<Option<String>, AppError> {
    match value {
        Some(v) if !v.is_empty() => Ok(Some(
            crate::services::crypto_service::decrypt(&v).await?,
        )),
        other => Ok(other),
    }
}
