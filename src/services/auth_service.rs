//! 认证服务：登录、登出、令牌刷新

use crate::{
    auth::jwt::{JwtService, TokenPair},
    auth::password::PasswordHasher,
    config::AppConfig,
    error::AppError,
    models::{audit::*, auth::*, user::*},
    repository::{auth_repo::AuthRepository, user_repo::UserRepository},
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub struct AuthService {
    db: PgPool,
    jwt_service: Arc<JwtService>,
    config: Arc<AppConfig>,
}

impl AuthService {
    pub fn new(db: PgPool, jwt_service: Arc<JwtService>, config: Arc<AppConfig>) -> Self {
        Self {
            db,
            jwt_service,
            config,
        }
    }

    /// 用户登录
    pub async fn login(
        &self,
        req: LoginRequest,
        client_ip: &str,
        user_agent: Option<&str>,
    ) -> Result<LoginResponse, AppError> {
        // 检查速率限制
        self.check_login_rate_limit(client_ip).await?;

        let user_repo = UserRepository::new(self.db.clone());
        let auth_repo = AuthRepository::new(self.db.clone());

        // 获取用户
        let user: User = user_repo
            .find_by_username(&req.username)
            .await?
            .ok_or(AppError::Unauthorized)?;

        // 检查账户状态
        self.check_account_status(&user)?;

        // 验证密码
        let hasher = PasswordHasher::new();
        hasher.verify(&req.password, &user.password_hash)?;

        // 检查账户是否被锁定
        if let Some(locked_until) = user.locked_until {
            if locked_until > chrono::Utc::now() {
                self.record_login_event(
                    None,
                    &req.username,
                    "login_failure",
                    Some("account_locked"),
                    client_ip,
                    user_agent,
                )
                .await;
                return Err(AppError::BadRequest("账户已被临时锁定".to_string()));
            }
        }

        // 重置失败次数
        if user.failed_login_attempts > 0 {
            let _ = user_repo.reset_failed_attempts(user.id).await;
        }

        // 获取用户角色和权限范围
        let (roles, scopes) = self.get_user_roles_and_scopes(user.id).await?;

        // 生成令牌
        let token_pair = self.jwt_service.generate_token_pair(
            &user.id,
            &user.username,
            roles.clone(),
            scopes,
        )?;

        // 存储刷新令牌
        let token_hash = AuthRepository::hash_token(&token_pair.refresh_token);
        let refresh_token = RefreshToken {
            id: Uuid::new_v4(),
            token_hash,
            user_id: user.id,
            device_id: None, // TODO: 生成设备指纹
            user_agent: user_agent.map(|s| s.to_string()),
            ip_address: client_ip.to_string(),
            expires_at: chrono::Utc::now()
                + chrono::Duration::seconds(self.config.security.refresh_token_exp_secs as i64),
            revoked_at: None,
            replaced_by: None,
            created_at: chrono::Utc::now(),
        };

        auth_repo.store_refresh_token(&refresh_token).await?;

        // 记录成功登录
        self.record_login_event(
            Some(user.id),
            &user.username,
            "login_success",
            None,
            client_ip,
            user_agent,
        )
        .await;

        Ok(LoginResponse {
            access_token: token_pair.access_token,
            refresh_token: token_pair.refresh_token,
            expires_in: token_pair.expires_in,
            user: UserResponse::from(user),
        })
    }

    /// 刷新令牌
    pub async fn refresh_token(
        &self,
        req: RefreshTokenRequest,
        client_ip: &str,
    ) -> Result<TokenPair, AppError> {
        // 验证刷新令牌
        let _claims = self
            .jwt_service
            .validate_refresh_token(&req.refresh_token)?;

        // 检查令牌是否被撤销
        let auth_repo = AuthRepository::new(self.db.clone());
        let token_hash = AuthRepository::hash_token(&req.refresh_token);
        let refresh_token_record: RefreshToken = auth_repo
            .find_refresh_token_by_hash(&token_hash)
            .await?
            .ok_or(AppError::Unauthorized)?;

        if refresh_token_record.revoked_at.is_some() {
            return Err(AppError::Unauthorized);
        }

        if refresh_token_record.expires_at < chrono::Utc::now() {
            return Err(AppError::Unauthorized);
        }

        // 获取用户
        let user_repo = UserRepository::new(self.db.clone());
        let user: User = user_repo
            .find_by_id(&refresh_token_record.user_id)
            .await?
            .ok_or(AppError::Unauthorized)?;

        // 检查账户状态
        self.check_account_status(&user)?;

        // 获取用户角色和权限范围
        let (roles, scopes) = self.get_user_roles_and_scopes(user.id).await?;

        // 生成新的令牌对
        let new_token_pair =
            self.jwt_service
                .generate_token_pair(&user.id, &user.username, roles, scopes)?;

        // 撤销旧的刷新令牌
        let _ = auth_repo
            .revoke_refresh_token(refresh_token_record.id)
            .await;

        // 存储新的刷新令牌
        let new_token_hash = AuthRepository::hash_token(&new_token_pair.refresh_token);
        let new_refresh_token = RefreshToken {
            id: Uuid::new_v4(),
            token_hash: new_token_hash,
            user_id: user.id,
            device_id: None,
            user_agent: None,
            ip_address: client_ip.to_string(),
            expires_at: chrono::Utc::now()
                + chrono::Duration::seconds(self.config.security.refresh_token_exp_secs as i64),
            revoked_at: None,
            replaced_by: Some(refresh_token_record.id),
            created_at: chrono::Utc::now(),
        };

        auth_repo.store_refresh_token(&new_refresh_token).await?;

        Ok(new_token_pair)
    }

    /// 登出（撤销刷新令牌）
    pub async fn logout(&self, refresh_token: &str, user_id: Uuid) -> Result<(), AppError> {
        let auth_repo = AuthRepository::new(self.db.clone());
        let token_hash = AuthRepository::hash_token(refresh_token);

        auth_repo
            .revoke_refresh_token_by_hash(&token_hash, user_id)
            .await?;

        Ok(())
    }

    /// 从所有设备登出
    pub async fn logout_all(&self, user_id: Uuid) -> Result<u64, AppError> {
        let auth_repo = AuthRepository::new(self.db.clone());
        auth_repo.revoke_all_refresh_tokens(user_id).await
    }

    /// 检查账户状态
    fn check_account_status(&self, user: &User) -> Result<(), AppError> {
        match user.status.as_str() {
            "disabled" => Err(AppError::BadRequest("账户已被禁用".to_string())),
            "locked" => Err(AppError::BadRequest("账户已被锁定".to_string())),
            "enabled" => Ok(()),
            _ => Err(AppError::Internal),
        }
    }

    /// 检查登录速率限制
    async fn check_login_rate_limit(&self, client_ip: &str) -> Result<(), AppError> {
        let auth_repo = AuthRepository::new(self.db.clone());

        // 检查最近的失败登录次数
        let recent_failures = auth_repo
            .count_recent_login_failures(client_ip, 300) // 5分钟
            .await?;

        if recent_failures >= 10 {
            tracing::warn!(
                %client_ip,
                recent_failures,
                "Rate limit exceeded for login"
            );
            return Err(AppError::RateLimitExceeded);
        }

        Ok(())
    }

    /// 获取用户的角色和权限范围
    async fn get_user_roles_and_scopes(
        &self,
        user_id: Uuid,
    ) -> Result<(Vec<String>, Vec<String>), AppError> {
        let user_repo = UserRepository::new(self.db.clone());
        let role_bindings: Vec<crate::models::role::RoleBinding> =
            user_repo.get_user_roles(user_id).await?;

        let roles: Vec<String> = role_bindings.iter().map(|r| r.role_name.clone()).collect();

        // 从角色绑定中收集权限范围
        let scopes: Vec<String> = role_bindings
            .iter()
            .map(|binding| match binding.scope_type.as_str() {
                "global" => "global".to_string(),
                "group" => format!("group:{}", binding.scope_value.as_deref().unwrap_or("*")),
                "environment" => format!("env:{}", binding.scope_value.as_deref().unwrap_or("*")),
                _ => "global".to_string(),
            })
            .collect();

        Ok((roles, scopes))
    }

    /// 记录登录事件
    async fn record_login_event(
        &self,
        user_id: Option<Uuid>,
        username: &str,
        event_type: &str,
        failure_reason: Option<&str>,
        source_ip: &str,
        user_agent: Option<&str>,
    ) {
        let auth_repo = AuthRepository::new(self.db.clone());

        let event = LoginEvent {
            id: Uuid::new_v4(),
            user_id,
            username: username.to_string(),
            event_type: event_type.to_string(),
            auth_method: "password".to_string(),
            failure_reason: failure_reason.map(|s| s.to_string()),
            source_ip: source_ip.to_string(),
            user_agent: user_agent.map(|s| s.to_string()),
            device_id: None,
            risk_tag: None, // TODO: 实现风险评估
            occurred_at: chrono::Utc::now(),
        };

        // 忽略审计日志错误，不要破坏请求流程
        let _ = auth_repo.record_login_event(&event).await;
    }
}
