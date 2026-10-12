//! 手动探针：向 ops.build 发布一条带 step_status 的 BuildStatusMessage，
//! 验证控制面 MQ 消费路径的步骤状态更新（需要本地 RabbitMQ 与运行中的 ops-service）。
use lapin::options::*;
use lapin::types::ShortString;
use lapin::{BasicProperties, Connection, ConnectionProperties};

#[tokio::test]
#[ignore = "需要本地 RabbitMQ 与运行中的 ops-service；手动运行：-- --ignored --nocapture <job_id>"]
async fn probe_step_status_message() {
    let Ok(job_id) = std::env::var("PROBE_JOB_ID") else {
        eprintln!("未设置 PROBE_JOB_ID，跳过探针（手动运行：PROBE_JOB_ID=<job_id> -- --ignored --nocapture）");
        return;
    };
    // --include-ignored 也会执行本测试：broker 不可达时优雅跳过而非失败
    let conn = match Connection::connect(
        "amqp://guest:guest@localhost:5672/%2F",
        ConnectionProperties::default(),
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("RabbitMQ 不可达（{e:?}），跳过探针");
            return;
        }
    };
    let ch = conn.create_channel().await.unwrap();
    ch.confirm_select(ConfirmSelectOptions::default()).await.unwrap();
    let msg = format!(r#"{{"task_id":"33333333-3333-3333-3333-333333333333","job_id":"{job_id}","runner_name":"demo-runner-01","status":"running","step_status":{{"step_id":"info","status":"succeeded","started_at":"2026-10-11T14:00:00Z","completed_at":"2026-10-11T14:00:05Z","exit_code":0}},"timestamp":"2026-10-11T15:10:00Z"}}"#);
    let confirm = ch.basic_publish(
        ShortString::from("ops.build"),
        ShortString::from("build.status.probe"),
        BasicPublishOptions::default(),
        msg.as_bytes(),
        BasicProperties::default().with_delivery_mode(2),
    ).await.unwrap().await.unwrap();
    println!("confirm: {confirm:?}");
}
