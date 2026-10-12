//! 发布确认互操作探针（需要本地 RabbitMQ：amqp://guest:guest@localhost:5672）
//!
//! 复现 runner 侧 "Broker nacked the status message"：
//! 同一 broker，服务端发布 ack、runner 发布 nack，但消息实际都投递成功。
//! 本探针分别测试：无消费者的纯净 channel vs 带消费者的 channel。

use std::sync::Arc;

use lapin::options::*;
use lapin::types::ShortString;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};

fn short_string(value: impl Into<String>) -> ShortString {
    value.into().into()
}

async fn setup_exchange(channel: &Channel) -> Result<(), Box<dyn std::error::Error>> {
    channel
        .exchange_declare(
            short_string("ops.build"),
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await?;
    Ok(())
}

async fn publish_and_report(
    channel: &Channel,
    label: &str,
    routing_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let confirm = channel
        .basic_publish(
            short_string("ops.build"),
            short_string(routing_key),
            BasicPublishOptions::default(),
            b"probe".as_slice(),
            BasicProperties::default().with_delivery_mode(2),
        )
        .await?
        .await?;
    println!("[{label}] routing_key={routing_key} confirm={confirm:?} is_ack={}", confirm.is_ack());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 RabbitMQ（amqp://guest:guest@localhost:5672），手动运行：-- --ignored --nocapture"]
async fn probe_publish_confirm() -> Result<(), Box<dyn std::error::Error>> {
    // --include-ignored 也会执行本测试：broker 不可达时优雅跳过而非失败
    let conn = match Connection::connect(
        "amqp://guest:guest@localhost:5672/%2F",
        ConnectionProperties::default(),
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("RabbitMQ 不可达（{:?}），跳过探针", e);
            return Ok(());
        }
    };

    // 场景 1：纯净 channel（无消费者）
    let plain: Arc<Channel> = Arc::new(conn.create_channel().await?);
    plain.confirm_select(ConfirmSelectOptions::default()).await?;
    setup_exchange(&plain).await?;
    publish_and_report(&plain, "plain", "build.status.probe-plain").await?;

    // 场景 2：带消费者的 channel（模拟 runner：consumer + publish 同 channel）
    let mixed: Arc<Channel> = Arc::new(conn.create_channel().await?);
    mixed.confirm_select(ConfirmSelectOptions::default()).await?;
    let queue = mixed
        .queue_declare(
            short_string("ops-runner.demo-runner-01.queue"),
            QueueDeclareOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await?;
    mixed
        .queue_bind(
            queue.name().clone(),
            short_string("ops.build"),
            short_string("build.status.probe-mixed"),
            QueueBindOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await?;
    let consumer = mixed
        .basic_consume(
            queue.name().clone(),
            short_string(format!("probe-{}", uuid::Uuid::new_v4())),
            BasicConsumeOptions::default(),
            lapin::types::FieldTable::default(),
        )
        .await?;
    // 消费者挂后台，只取一帧即丢弃，保持 channel 上有活跃消费者
    tokio::spawn(async move {
        use futures_util::StreamExt;
        let mut consumer = consumer;
        while let Some(Ok(delivery)) = consumer.next().await {
            let _ = delivery.acker.ack(BasicAckOptions::default()).await;
        }
    });
    publish_and_report(&mixed, "mixed", "build.status.probe-mixed").await?;
    publish_and_report(&mixed, "mixed-2", "build.status.probe-mixed").await?;

    Ok(())
}
