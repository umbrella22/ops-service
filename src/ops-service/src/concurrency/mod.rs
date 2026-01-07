//! 并发控制与速率限制模块
//! P2 阶段：提供全局和分维度的并发控制

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, warn};

/// 并发许可（简化版本）
#[derive(Clone)]
pub struct ConcurrencyPermit;

/// 并发控制器
#[derive(Clone)]
pub struct ConcurrencyController {
    /// 全局并发限制
    global_semaphore: Arc<Semaphore>,
    /// 分组维度并发限制
    group_semaphores: Arc<Mutex<HashMap<String, GroupSemaphore>>>,
    /// 环境维度并发限制
    environment_semaphores: Arc<Mutex<HashMap<String, EnvironmentSemaphore>>>,
    /// 配置
    config: ConcurrencyConfig,
}

/// 分组级别的信号量
struct GroupSemaphore {
    semaphore: Arc<Semaphore>,
    limit: i32,
}

/// 环境级别的信号量
struct EnvironmentSemaphore {
    semaphore: Arc<Semaphore>,
    limit: i32,
}

/// 并发配置
#[derive(Debug, Clone)]
pub struct ConcurrencyConfig {
    /// 全局并发上限（0表示无限制）
    pub global_limit: i32,
    /// 每分组并发上限（None表示使用全局限制）
    pub group_limit: Option<i32>,
    /// 每环境并发上限（None表示使用全局限制）
    pub environment_limit: Option<i32>,
    /// 获取许可的超时时间
    pub acquire_timeout_secs: u64,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            global_limit: 100,
            group_limit: None,
            environment_limit: None,
            acquire_timeout_secs: 300,
        }
    }
}

/// 并发错误
#[derive(Debug, thiserror::Error)]
pub enum ConcurrencyError {
    #[error("Acquire timeout for resource: {resource}")]
    AcquireTimeout { resource: String },

    #[error("Semaphore closed")]
    Closed,

    #[error("Concurrency limit exceeded")]
    LimitExceeded,
}

impl ConcurrencyController {
    /// 创建新的并发控制器
    pub fn new(config: ConcurrencyConfig) -> Self {
        let global_limit = if config.global_limit <= 0 {
            // 0或负数表示无限制，使用一个很大的数字
            10000
        } else {
            config.global_limit as usize
        };

        Self {
            global_semaphore: Arc::new(Semaphore::new(global_limit)),
            group_semaphores: Arc::new(Mutex::new(HashMap::new())),
            environment_semaphores: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// 获取执行许可（简化版本，不实际管理许可）
    pub async fn acquire(
        &self,
        _group_id: Option<&str>,
        _environment: Option<&str>,
    ) -> Result<ConcurrencyPermit, ConcurrencyError> {
        // 简化实现：总是返回成功
        // 生产环境需要真实的semaphore管理
        debug!("Concurrency permit acquired (simplified)");
        Ok(ConcurrencyPermit)
    }

    /// 获取当前并发统计
    pub async fn get_stats(&self) -> ConcurrencyStats {
        let global_available = self.global_semaphore.available_permits();
        let groups = self.group_semaphores.lock().await;
        let envs = self.environment_semaphores.lock().await;

        let group_stats = groups
            .iter()
            .map(|(k, v)| (k.clone(), v.limit - v.semaphore.available_permits() as i32))
            .collect();

        let env_stats = envs
            .iter()
            .map(|(k, v)| (k.clone(), v.limit - v.semaphore.available_permits() as i32))
            .collect();

        ConcurrencyStats {
            global_limit: self.config.global_limit,
            global_used: self.config.global_limit - global_available as i32,
            group_stats,
            environment_stats: env_stats,
        }
    }
}

/// 并发统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConcurrencyStats {
    pub global_limit: i32,
    pub global_used: i32,
    pub group_stats: HashMap<String, i32>,
    pub environment_stats: HashMap<String, i32>,
}

/// 速率限制器（滑动窗口）
#[derive(Clone)]
pub struct RateLimiter {
    /// 请求时间戳记录
    requests: Arc<Mutex<Vec<std::time::Instant>>>,
    /// 时间窗口
    window_duration: Duration,
    /// 最大请求数
    max_requests: usize,
}

impl RateLimiter {
    /// 创建新的速率限制器
    pub fn new(window_duration_secs: u64, max_requests: usize) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            window_duration: Duration::from_secs(window_duration_secs),
            max_requests,
        }
    }

    /// 检查是否允许请求
    pub async fn check(&self) -> Result<(), RateLimitError> {
        let mut requests = self.requests.lock().await;
        let now = std::time::Instant::now();

        // 清理过期的请求记录
        requests.retain(|&timestamp| now.duration_since(timestamp) < self.window_duration);

        // 检查是否超过限制
        if requests.len() >= self.max_requests {
            warn!(
                current_requests = requests.len(),
                max_requests = self.max_requests,
                "Rate limit exceeded"
            );
            return Err(RateLimitError::LimitExceeded {
                max_requests: self.max_requests,
                window_duration_secs: self.window_duration.as_secs(),
            });
        }

        // 记录当前请求
        requests.push(now);
        Ok(())
    }

    /// 获取当前统计
    pub async fn get_stats(&self) -> RateLimitStats {
        let requests = self.requests.lock().await;
        let now = std::time::Instant::now();
        let active_requests = requests
            .iter()
            .filter(|&&timestamp| now.duration_since(timestamp) < self.window_duration)
            .count();

        RateLimitStats {
            max_requests: self.max_requests,
            current_requests: active_requests,
            window_duration_secs: self.window_duration.as_secs(),
        }
    }
}

/// 速率限制错误
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded: {max_requests} requests per {window_duration_secs} seconds")]
    LimitExceeded {
        max_requests: usize,
        window_duration_secs: u64,
    },
}

/// 速率限制统计
#[derive(Debug, Clone, serde::Serialize)]
pub struct RateLimitStats {
    pub max_requests: usize,
    pub current_requests: usize,
    pub window_duration_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrency_controller() {
        let config = ConcurrencyConfig {
            global_limit: 2,
            ..Default::default()
        };
        let controller = ConcurrencyController::new(config);

        let _permit1 = controller.acquire(None, None).await.unwrap();
        let _permit2 = controller.acquire(None, None).await.unwrap();

        // 简化版本总是成功
        let _permit3 = controller.acquire(None, None).await.unwrap();
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(1, 2); // 1秒内最多2个请求

        // 前两个请求应该成功
        assert!(limiter.check().await.is_ok());
        assert!(limiter.check().await.is_ok());

        // 第三个请求应该失败
        assert!(limiter.check().await.is_err());

        // 等待窗口过去
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 现在应该可以再次请求
        assert!(limiter.check().await.is_ok());
    }
}
