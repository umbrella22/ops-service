//! RabbitMQ 发布器
//!
//! 负责将构建任务派发到 RabbitMQ，供 Runner 消费执行

use anyhow::{Context, Result};
use futures::pin_mut;
use lapin::types::ShortString;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind, options::*};
use secrecy::ExposeSecret;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::RabbitMqConfig;

fn short_string(value: impl Into<String>) -> ShortString {
    value.into().into()
}

/// RabbitMQ 发布器
#[derive(Clone)]
pub struct RabbitMqPublisher {
    config: Arc<RabbitMqConfig>,
    #[allow(dead_code)]
    connection: Arc<Connection>,
    channel: Arc<Channel>,
}

impl RabbitMqPublisher {
    /// 创建新的发布器
    pub async fn new(config: RabbitMqConfig) -> Result<Self> {
        let amqp_url = config.amqp_url.expose_secret();
        info!("Connecting to RabbitMQ: {}", amqp_url.replace(':', ":***@"));

        let conn = Connection::connect(amqp_url, ConnectionProperties::default())
            .await
            .context("Failed to connect to RabbitMQ")?;

        info!("Connected to RabbitMQ");

        let channel = conn
            .create_channel()
            .await
            .context("Failed to create channel")?;

        // 设置发布者确认
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .context("Failed to enable publisher confirms")?;

        info!("RabbitMQ publisher created with confirms enabled");

        Ok(Self {
            config: Arc::new(config),
            connection: Arc::new(conn),
            channel: Arc::new(channel),
        })
    }

    /// 声明交换机和队列
    pub async fn setup_infrastructure(&self) -> Result<()> {
        // 声明构建任务交换机（Topic 类型）
        self.channel
            .exchange_declare(
                short_string(self.config.build_exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to declare build exchange")?;

        info!("Declared build exchange: {}", self.config.build_exchange);

        // 声明 Runner 交换机（Direct 类型，用于注册/心跳）
        self.channel
            .exchange_declare(
                short_string(self.config.runner_exchange.clone()),
                ExchangeKind::Direct,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to declare runner exchange")?;

        info!("Declared runner exchange: {}", self.config.runner_exchange);

        // 声明死信交换机
        let dlq_exchange = format!("{}.dlq", self.config.build_exchange);
        self.channel
            .exchange_declare(
                short_string(dlq_exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to declare DLQ exchange")?;

        info!("Declared DLQ exchange: {}", dlq_exchange);

        Ok(())
    }

    /// 发布构建任务
    pub async fn publish_build_task(
        &self,
        build_type: &str,
        runner_name: Option<&str>,
        payload: &[u8],
    ) -> Result<()> {
        // 构建路由键：build.<type>[.<runner_name>]
        let routing_key = if let Some(runner) = runner_name {
            format!("build.{}.{}", build_type, runner)
        } else {
            format!("build.{}", build_type)
        };

        let confirm = self
            .channel
            .basic_publish(
                short_string(self.config.build_exchange.clone()),
                short_string(routing_key.clone()),
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default()
                    .with_delivery_mode(2) // 持久化
                    .with_content_type("application/json".into()),
            )
            .await?
            .await?;

        if confirm.is_ack() {
            debug!("Build task published and acknowledged: {}", routing_key);
        } else {
            warn!("Build task published but not acknowledged: {}", routing_key);
        }

        Ok(())
    }

    /// 发布构建取消消息（广播）
    ///
    /// 路由键 `build.cancel.<job_id>`；所有 runner 的任务队列绑定
    /// `build.cancel.#`，由各 runner 按 job_id 匹配本地活跃任务。
    pub async fn publish_build_cancel(&self, job_id: uuid::Uuid, payload: &[u8]) -> Result<()> {
        let routing_key = format!("build.cancel.{}", job_id);

        let confirm = self
            .channel
            .basic_publish(
                short_string(self.config.build_exchange.clone()),
                short_string(routing_key.clone()),
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default()
                    .with_delivery_mode(2) // 持久化：broker 重启后未消费的取消不丢失
                    .with_content_type("application/json".into()),
            )
            .await?
            .await?;

        if confirm.is_ack() {
            debug!("Build cancel published: {}", routing_key);
        } else {
            warn!("Build cancel published but not acknowledged: {}", routing_key);
        }

        Ok(())
    }

    /// 发布到 Runner 交换机（用于注册/心跳响应等）
    pub async fn publish_to_runner(
        &self,
        routing_key: &str,
        payload: impl Serialize,
    ) -> Result<()> {
        let data = serde_json::to_vec(&payload).context("Failed to serialize runner message")?;

        self.channel
            .basic_publish(
                short_string(self.config.runner_exchange.clone()),
                short_string(routing_key),
                BasicPublishOptions::default(),
                &data,
                BasicProperties::default()
                    .with_delivery_mode(1) // 非持久化
                    .with_content_type("application/json".into()),
            )
            .await?;

        debug!("Published to runner exchange: {}", routing_key);
        Ok(())
    }

    /// 健康检查
    pub async fn health_check(&self) -> bool {
        match self
            .channel
            .exchange_declare(
                short_string(self.config.build_exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
        {
            Err(e) => {
                warn!("RabbitMQ health check failed: {}", e);
                false
            }
            _ => true,
        }
    }
}

/// RabbitMQ 发布器池
pub struct RabbitMqPublisherPool {
    publisher: Arc<RwLock<Option<RabbitMqPublisher>>>,
    config: RabbitMqConfig,
}

impl RabbitMqPublisherPool {
    /// 创建新的发布器池
    pub fn new(config: RabbitMqConfig) -> Self {
        Self {
            publisher: Arc::new(RwLock::new(None)),
            config,
        }
    }

    /// 获取或初始化发布器
    pub async fn get(&self) -> Result<RabbitMqPublisher> {
        // 检查是否有可用的发布器
        {
            let reader = self.publisher.read().await;
            if let Some(publisher) = reader.as_ref() {
                // 检查健康状态
                if publisher.health_check().await {
                    return Ok(publisher.clone());
                }
            }
        }

        // 需要重新初始化
        let mut writer = self.publisher.write().await;
        let new_publisher = RabbitMqPublisher::new(self.config.clone()).await?;
        new_publisher.setup_infrastructure().await?;
        *writer = Some(new_publisher.clone());
        Ok(new_publisher)
    }

    /// 健康检查
    pub async fn health_check(&self) -> bool {
        let reader = self.publisher.read().await;
        if let Some(publisher) = reader.as_ref() {
            publisher.health_check().await
        } else {
            false
        }
    }
}

/// RabbitMQ 消费器
/// 用于消费 Runner 回传的状态和日志消息
#[derive(Clone)]
pub struct RabbitMqConsumer {
    config: Arc<RabbitMqConfig>,
    #[allow(dead_code)]
    connection: Arc<Connection>,
    channel: Arc<Channel>,
}

impl RabbitMqConsumer {
    /// 创建新的消费者
    pub async fn new(config: RabbitMqConfig) -> Result<Self> {
        let amqp_url = config.amqp_url.expose_secret();
        info!("Connecting to RabbitMQ for consumer: {}", amqp_url.replace(':', ":***@"));

        let conn = Connection::connect(amqp_url, ConnectionProperties::default())
            .await
            .context("Failed to connect to RabbitMQ")?;

        info!("Connected to RabbitMQ for consumer");

        let channel = conn
            .create_channel()
            .await
            .context("Failed to create consumer channel")?;

        // 设置 QoS（每次只获取一条消息，确保顺序处理）
        channel
            .basic_qos(1, BasicQosOptions::default())
            .await
            .context("Failed to set QoS")?;

        info!("RabbitMQ consumer created with QoS=1");

        Ok(Self {
            config: Arc::new(config),
            connection: Arc::new(conn),
            channel: Arc::new(channel),
        })
    }

    /// 声明消费用的队列和绑定
    pub async fn setup_consumer_queues(&self) -> Result<()> {
        // 声明构建状态交换机
        self.channel
            .exchange_declare(
                short_string(self.config.build_exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to declare build exchange")?;

        // 声明死信交换机（毒消息的最终归宿）
        let dlq_exchange = format!("{}.dlq", self.config.build_exchange);
        self.channel
            .exchange_declare(
                short_string(dlq_exchange.clone()),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to declare DLQ exchange")?;

        // 主队列统一挂上死信路由：reject(requeue=false) 的消息进入 DLQ
        let mut queue_args = lapin::types::FieldTable::default();
        queue_args.insert(
            "x-dead-letter-exchange".into(),
            lapin::types::AMQPValue::LongString(dlq_exchange.clone().into()),
        );

        // 声明状态更新队列（Fanout from build status）
        let status_queue = "build.status.queue";
        let _queue = self
            .channel
            .queue_declare(
                short_string(status_queue),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                queue_args.clone(),
            )
            .await
            .context("Failed to declare status queue")?;

        // 绑定状态队列到交换机（监听所有状态更新）
        self.channel
            .queue_bind(
                short_string(status_queue),
                short_string(self.config.build_exchange.clone()),
                short_string("build.status.#"),
                QueueBindOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to bind status queue")?;

        info!("Declared and bound status queue: {}", status_queue);

        // 声明日志队列
        let log_queue = "build.log.queue";
        let _queue = self
            .channel
            .queue_declare(
                short_string(log_queue),
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                queue_args.clone(),
            )
            .await
            .context("Failed to declare log queue")?;

        // 绑定日志队列到交换机
        self.channel
            .queue_bind(
                short_string(log_queue),
                short_string(self.config.build_exchange.clone()),
                short_string("build.log.#"),
                QueueBindOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .context("Failed to bind log queue")?;

        info!("Declared and bound log queue: {}", log_queue);

        // 声明 DLQ 队列并绑定，确保死信有落脚点（否则会被 broker 丢弃）
        for (dlq_name, routing) in [
            ("build.status.dlq", "build.status.#"),
            ("build.log.dlq", "build.log.#"),
        ] {
            self.channel
                .queue_declare(
                    short_string(dlq_name),
                    QueueDeclareOptions {
                        durable: true,
                        ..Default::default()
                    },
                    lapin::types::FieldTable::default(),
                )
                .await
                .with_context(|| format!("Failed to declare dead letter queue {}", dlq_name))?;
            self.channel
                .queue_bind(
                    short_string(dlq_name),
                    short_string(dlq_exchange.clone()),
                    short_string(routing),
                    QueueBindOptions::default(),
                    lapin::types::FieldTable::default(),
                )
                .await
                .with_context(|| format!("Failed to bind dead letter queue {}", dlq_name))?;
            info!("Declared and bound dead letter queue: {}", dlq_name);
        }

        Ok(())
    }

    /// 消费队列的通用循环：处理成功才 ack；
    /// 处理失败首次 requeue 重试，redelivery 仍失败则路由到 DLQ。
    async fn consume_with_handler<F, Fut>(&self, queue: &str, mut handler: F) -> Result<()>
    where
        F: FnMut(Vec<u8>, lapin::message::Delivery) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        // consumer tag 必须在 channel 内唯一：status/log 两个消费者共用
        // 同一 channel，固定同名 tag 会让第二个被 broker 以 NOT_ALLOWED 拒绝
        let consumer_tag = format!("{}_{}", queue.replace('.', "_"), uuid::Uuid::new_v4());
        let consumer = self
            .channel
            .basic_consume(
                short_string(queue),
                short_string(consumer_tag),
                BasicConsumeOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .with_context(|| format!("Failed to create consumer for {}", queue))?;

        info!("Started consuming messages from: {}", queue);

        use futures::StreamExt;
        pin_mut!(consumer);

        while let Some(delivery_result) = consumer.next().await {
            let delivery = match delivery_result {
                Ok(d) => d,
                Err(e) => {
                    tracing::error!("Consumer error on {}: {}", queue, e);
                    // 连接/通道级错误：返回 Err 让上层重建连接
                    return Err(anyhow::Error::new(e).context("consumer stream error"));
                }
            };

            let data = delivery.data.clone();
            let delivery_tag = delivery.delivery_tag;
            let redelivered = delivery.redelivered;
            match handler(data, delivery).await {
                Ok(()) => {
                    if let Err(e) = self
                        .channel
                        .basic_ack(delivery_tag, BasicAckOptions::default())
                        .await
                    {
                        tracing::error!("Failed to ack message on {}: {}", queue, e);
                    }
                }
                Err(e) => {
                    // 首次投递失败 → requeue 重试一次；redelivery 仍失败 → 进 DLQ
                    if redelivered {
                        tracing::error!(
                            queue = %queue,
                            error = %e,
                            "Handler failed on redelivered message, routing to DLQ"
                        );
                        let _ = self
                            .channel
                            .basic_reject(delivery_tag, BasicRejectOptions { requeue: false })
                            .await;
                    } else {
                        tracing::warn!(
                            queue = %queue,
                            error = %e,
                            "Handler failed, requeueing for one retry"
                        );
                        let _ = self
                            .channel
                            .basic_nack(delivery_tag, BasicNackOptions { requeue: true, ..Default::default() })
                            .await;
                    }
                }
            }
        }

        Ok(())
    }

    /// 启动状态消息消费者
    pub async fn consume_status_messages<F, Fut>(&self, handler: F) -> Result<()>
    where
        F: FnMut(Vec<u8>, lapin::message::Delivery) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        self.consume_with_handler("build.status.queue", handler).await
    }

    /// 启动日志消息消费者
    pub async fn consume_log_messages<F, Fut>(&self, handler: F) -> Result<()>
    where
        F: FnMut(Vec<u8>, lapin::message::Delivery) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        self.consume_with_handler("build.log.queue", handler).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn create_test_config() -> RabbitMqConfig {
        RabbitMqConfig {
            amqp_url: SecretString::from("amqp://guest:guest@localhost:5672/%2F".to_string()),
            vhost: "/".to_string(),
            build_exchange: "test.ops.build".to_string(),
            runner_exchange: "test.ops.runner".to_string(),
            pool_size: 1,
            publish_timeout_secs: 5,
        }
    }

    #[test]
    fn test_config_creation() {
        let config = create_test_config();
        assert_eq!(config.build_exchange, "test.ops.build");
        assert_eq!(config.runner_exchange, "test.ops.runner");
    }

    #[test]
    fn test_routing_key_generation() {
        let routing_key = format!("build.{}", "node");
        assert_eq!(routing_key, "build.node");

        let routing_key = format!("build.{}.{}", "node", "runner-1");
        assert_eq!(routing_key, "build.node.runner-1");
    }
}
