//! 终态通知服务 (P4)
//!
//! 构建作业/SSH 作业到达终态时，向配置的群机器人 webhook 推送文本通知。
//! 支持飞书 / 钉钉 / 企业微信 / Slack 四种消息格式。
//!
//! 设计约束：
//! - fire-and-forget：通知失败只记日志，绝不影响主流程；
//! - 全局配置驱动（OPS_NOTIFICATIONS__*），不做每用户规则（后续可扩展）。

use secrecy::ExposeSecret;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// 通知渠道类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    Feishu,
    DingTalk,
    WeCom,
    Slack,
}

impl NotifyKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "feishu" | "lark" => Some(Self::Feishu),
            "dingtalk" => Some(Self::DingTalk),
            "wecom" | "wechat" => Some(Self::WeCom),
            "slack" => Some(Self::Slack),
            _ => None,
        }
    }
}

/// 终态通知服务
pub struct NotifyService {
    webhook_url: String,
    kind: NotifyKind,
    client: reqwest::Client,
    timeout: Duration,
}

impl NotifyService {
    /// 从配置构造；enabled=false 或 url 缺失时返回 None（调用方以 Option 持有）
    pub fn from_config(config: &crate::config::NotifyConfig) -> Option<Arc<Self>> {
        if !config.enabled {
            return None;
        }
        let url = config
            .webhook_url
            .as_ref()
            .map(|s| s.expose_secret().to_string())
            .unwrap_or_default();
        if url.is_empty() {
            tracing::warn!(
                "OPS_NOTIFICATIONS__ENABLED=true but webhook_url is empty; notifications disabled"
            );
            return None;
        }
        let kind = match NotifyKind::parse(&config.kind) {
            Some(k) => k,
            None => {
                tracing::warn!(
                    kind = %config.kind,
                    "Unknown notification kind; expected feishu/dingtalk/wecom/slack; notifications disabled"
                );
                return None;
            }
        };
        Some(Arc::new(Self {
            webhook_url: url,
            kind,
            client: reqwest::Client::new(),
            timeout: Duration::from_secs(config.timeout_secs.max(1)),
        }))
    }

    /// 推送终态文本通知（fire-and-forget：内部吞掉所有错误）
    pub async fn notify_terminal(&self, title: &str, detail: &str) {
        let text = format!("{title}\n{detail}");
        let payload = match self.kind {
            // 飞书自定义机器人
            NotifyKind::Feishu => json!({"msg_type": "text", "content": {"text": text}}),
            // 钉钉自定义机器人
            NotifyKind::DingTalk => json!({"msgtype": "text", "text": {"content": text}}),
            // 企业微信群机器人
            NotifyKind::WeCom => json!({"msgtype": "text", "text": {"content": text}}),
            // Slack Incoming Webhook
            NotifyKind::Slack => json!({"text": text}),
        };

        let result = self
            .client
            .post(&self.webhook_url)
            .timeout(self.timeout)
            .json(&payload)
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                debug!("Terminal notification sent: {}", title);
            }
            Ok(resp) => {
                warn!(
                    status = %resp.status(),
                    "Notification webhook returned non-success"
                );
            }
            Err(e) => {
                warn!(error = %e, "Failed to send terminal notification");
            }
        }
    }
}
