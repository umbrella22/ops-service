//! 配置系统
//! 从环境变量加载所有配置，使用 Secret 包装敏感信息

use config::{Config, ConfigError, Environment};
use secrecy::{ExposeSecret, Secret};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// 监听地址，例如 "0.0.0.0:3000"
    pub addr: String,
    /// 优雅关闭超时时间（秒）
    pub graceful_shutdown_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// 数据库连接 URL（使用 Secret 包装，防止日志泄露）
    pub url: Secret<String>,
    /// 最大连接数
    pub max_connections: u32,
    /// 最小连接数
    pub min_connections: u32,
    /// 获取连接超时时间（秒）
    pub acquire_timeout_secs: u64,
    /// 空闲连接超时时间（秒）
    pub idle_timeout_secs: u64,
    /// 连接最大生命周期（秒）
    pub max_lifetime_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// 日志级别: trace, debug, info, warn, error
    pub level: String,
    /// 日志格式: json, pretty
    pub format: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    /// JWT 密钥（使用 Secret 包装，防止日志泄露）
    pub jwt_secret: Secret<String>,
    /// 访问令牌过期时间（秒）
    pub access_token_exp_secs: u64,
    /// 刷新令牌过期时间（秒）
    pub refresh_token_exp_secs: u64,
    /// 密码最小长度
    pub password_min_length: usize,
    /// 密码必须包含大写字母
    pub password_require_uppercase: bool,
    /// 密码必须包含数字
    pub password_require_digit: bool,
    /// 密码必须包含特殊字符
    pub password_require_special: bool,
    /// 最大登录失败次数
    pub max_login_attempts: u32,
    /// 登录锁定持续时间（秒）
    pub login_lockout_duration_secs: u64,
    /// 速率限制（每秒请求数）
    pub rate_limit_rps: u64,
    /// 是否信任 X-Forwarded-For 头
    pub trust_proxy: bool,
    /// IP 白名单（可选）
    pub allowed_ips: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    pub security: SecurityConfig,
}

impl AppConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut settings = Config::builder();

        // 添加默认配置
        settings = settings
            .set_default("server.addr", "0.0.0.0:3000")?
            .set_default("server.graceful_shutdown_timeout_secs", 30)?
            .set_default("database.max_connections", 10)?
            .set_default("database.min_connections", 2)?
            .set_default("database.acquire_timeout_secs", 30)?
            .set_default("database.idle_timeout_secs", 600)?
            .set_default("database.max_lifetime_secs", 1800)?
            .set_default("logging.level", "info")?
            .set_default("logging.format", "json")?
            .set_default("security.jwt_secret", "change-this-secret-in-production-min-32-chars!")?
            .set_default("security.access_token_exp_secs", 900)?
            .set_default("security.refresh_token_exp_secs", 604800)?
            .set_default("security.password_min_length", 8)?
            .set_default("security.password_require_uppercase", true)?
            .set_default("security.password_require_digit", true)?
            .set_default("security.password_require_special", false)?
            .set_default("security.max_login_attempts", 5)?
            .set_default("security.login_lockout_duration_secs", 1800)?
            .set_default("security.rate_limit_rps", 100)?
            .set_default("security.trust_proxy", true)?;

        // 从环境变量加载配置（前缀为 OPS_）
        settings = settings.add_source(
            Environment::with_prefix("OPS")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true),
        );

        let config: AppConfig = settings.build()?.try_deserialize()?;

        // 验证配置
        config.validate()?;

        Ok(config)
    }

    /// 验证配置合法性
    fn validate(&self) -> Result<(), ConfigError> {
        // 验证端口范围
        if let Some(port_str) = self.server.addr.split(':').next_back() {
            if let Ok(port) = port_str.parse::<u16>() {
                if port < 1024 {
                    return Err(ConfigError::Message("Server port should be >= 1024".to_string()));
                }
            }
        }

        // 验证日志级别
        match self.logging.level.to_lowercase().as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => {
                return Err(ConfigError::Message(format!(
                    "Invalid log level: {}. Must be one of: trace, debug, info, warn, error",
                    self.logging.level
                )))
            }
        }

        // 验证日志格式
        match self.logging.format.to_lowercase().as_str() {
            "json" | "pretty" => {}
            _ => {
                return Err(ConfigError::Message(format!(
                    "Invalid log format: {}. Must be one of: json, pretty",
                    self.logging.format
                )))
            }
        }

        // 验证数据库连接池配置
        if self.database.max_connections < self.database.min_connections {
            return Err(ConfigError::Message(
                "max_connections must be >= min_connections".to_string(),
            ));
        }

        // 验证 JWT 密钥长度（至少 32 字符）
        if self.security.jwt_secret.expose_secret().len() < 32 {
            return Err(ConfigError::Message(
                "JWT secret must be at least 32 characters long".to_string(),
            ));
        }

        // 验证令牌过期时间
        if self.security.access_token_exp_secs < 60 || self.security.access_token_exp_secs > 86400 {
            return Err(ConfigError::Message(
                "access_token_exp_secs must be between 60 and 86400 (1 minute to 24 hours)"
                    .to_string(),
            ));
        }

        if self.security.refresh_token_exp_secs < 3600
            || self.security.refresh_token_exp_secs > 2592000
        {
            return Err(ConfigError::Message(
                "refresh_token_exp_secs must be between 3600 and 2592000 (1 hour to 30 days)"
                    .to_string(),
            ));
        }

        // 验证密码策略
        if self.security.password_min_length < 6 || self.security.password_min_length > 128 {
            return Err(ConfigError::Message(
                "password_min_length must be between 6 and 128".to_string(),
            ));
        }

        // 验证登录失败锁定配置
        if self.security.max_login_attempts < 1 || self.security.max_login_attempts > 20 {
            return Err(ConfigError::Message(
                "max_login_attempts must be between 1 and 20".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_config_defaults() {
        // 清理所有可能的环境变量
        std::env::remove_var("OPS_DATABASE__URL");
        std::env::remove_var("OPS_SERVER__ADDR");
        std::env::remove_var("OPS_LOGGING__LEVEL");
        std::env::remove_var("OPS_LOGGING__FORMAT");
        std::env::remove_var("OPS_SECURITY__JWT_SECRET");

        // 设置测试环境变量
        std::env::set_var("OPS_DATABASE__URL", "postgresql://user:pass@localhost/db");

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.server.addr, "0.0.0.0:3000");
        assert_eq!(config.logging.level, "info");

        std::env::remove_var("OPS_DATABASE__URL");
    }

    #[test]
    #[serial]
    fn test_config_validation_invalid_port() {
        // 清理环境变量
        std::env::remove_var("OPS_SERVER__ADDR");
        std::env::remove_var("OPS_SERVER__ADDR");
        std::env::remove_var("OPS_DATABASE__URL");
        std::env::remove_var("OPS_DATABASE__URL");

        std::env::set_var("OPS_SERVER__ADDR", "0.0.0.0:80");
        std::env::set_var("OPS_DATABASE__URL", "postgresql://user:pass@localhost/db");

        let result = AppConfig::from_env();
        assert!(result.is_err());

        std::env::remove_var("OPS_SERVER__ADDR");
        std::env::remove_var("OPS_DATABASE__URL");
    }

    #[test]
    #[serial]
    fn test_config_validation_invalid_log_level() {
        // 清理环境变量
        std::env::remove_var("OPS_LOGGING__LEVEL");
        std::env::remove_var("OPS_LOGGING__LEVEL");
        std::env::remove_var("OPS_DATABASE__URL");
        std::env::remove_var("OPS_DATABASE__URL");

        std::env::set_var("OPS_LOGGING__LEVEL", "invalid");
        std::env::set_var("OPS_DATABASE__URL", "postgresql://user:pass@localhost/db");

        let result = AppConfig::from_env();
        assert!(result.is_err());

        std::env::remove_var("OPS_LOGGING__LEVEL");
        std::env::remove_var("OPS_DATABASE__URL");
    }
}
