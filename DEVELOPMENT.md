# 开发环境设置指南

## P4 特性速览（Pipeline-as-code / Git 触发 / 实时日志 / 通知）

- **Pipeline-as-code**：仓库根目录放 `.ops.yml`，创建构建时不传 `steps` 只传
  `pipeline_file`（缺省 `.ops.yml`），Runner 在 clone 后解析并执行。
  完整字段说明见 `docs/pipeline.md`。
- **Git push 自动触发**：`POST /api/v1/build-triggers` 创建触发器
  （仓库 + 分支 + 签名密钥），Git 服务 push webhook 打到
  `POST /api/v1/webhooks/git/{trigger_id}`，验签通过即自动创建
  管道模式构建。GitHub 用 HMAC-SHA256 签名，GitLab 用 Token 头。
- **实时日志 tail**：`GET /api/v1/stream/builds/{id}`（SSE）推送
  作业/步骤状态与实时日志；需要 `build:output_detail` 权限，
  非管理员仅可订阅自己触发的构建。
- **终态通知**：`OPS_NOTIFICATIONS__ENABLED=true` +
  `OPS_NOTIFICATIONS__WEBHOOK_URL=...` + `OPS_NOTIFICATIONS__KIND=feishu|dingtalk|wecom|slack`，
  构建到达终态时推送文本通知（fire-and-forget，不影响主流程）。

### 多实例约束（重要）

当前版本控制面为**单实例设计**：SSH 作业的并发许可
（`ConcurrencyController`）与实时事件总线（`EventBus`/SSE）均为进程内状态。
水平部署多个控制面实例时：

- 构建派发链路（Runner 调度/容量认领/取消）**是多实例安全的**——
  全部以 PostgreSQL + RabbitMQ 为准（原子认领 SQL、广播取消）；
- SSH 作业的并发上限会退化为"每实例各自限额"；
- SSE 订阅只能收到本实例发布的事件。

跨实例并发控制与事件总线分布式化（DB/Redis backing）为后续迭代方向。

## 环境要求

- **Rust 1.99+**（edition 2024，MSRV 声明在各 crate 的 `rust-version`）
- Docker（用于本地 PostgreSQL，见下文）
- 可选：RabbitMQ（构建派发功能；未配置时服务仍可启动，相关功能降级）

## 环境变量配置

本项目使用 `dotenv` 加载环境变量，支持多环境配置。

### 环境变量加载优先级

应用程序按以下优先级加载环境变量文件：

1. `.env.local` - 本地覆盖配置（不提交到版本控制）
2. `.env.development` - 开发环境配置
3. `.env` - 默认配置

### 环境变量命名规则

配置使用 `OPS_` 前缀，嵌套字段使用双下划线 `__` 分隔：

```bash
# 服务器配置
OPS_SERVER__ADDR=127.0.0.1:3000

# 数据库配置
OPS_DATABASE__URL=postgresql://user:pass@localhost/db
OPS_DATABASE__MAX_CONNECTIONS=10

# 日志配置
OPS_LOGGING__LEVEL=debug
OPS_LOGGING__FORMAT=pretty

# 安全配置
OPS_SECURITY__JWT_SECRET=your-secret-key-min-32-chars
OPS_SECURITY__RATE_LIMIT_RPS=100
```

## 快速开始

### 1. 启动 Docker 数据库（推荐）

```bash
docker-compose -f docker-compose.dev.yml up -d
```

### 2. 运行应用

```bash
# 开发模式（自动加载 .env.development）
cargo run

# 或指定环境
OPS_ENV=development cargo run
```

### 3. 运行测试

```bash
cargo test
```

## 开发环境配置文件

### `.env.development` - 开发环境

```bash
# 使用本地数据库
OPS_DATABASE__URL=postgresql://ops_user:dev_password@localhost:5432/ops_service
OPS_LOGGING__LEVEL=debug
OPS_LOGGING__FORMAT=pretty
OPS_SECURITY__RATE_LIMIT_RPS=1000  # 开发环境不限制速率
```

### `.env.production` - 生产环境（参考）

```bash
# 使用生产数据库
OPS_DATABASE__URL=postgresql://ops_user:strong_password@postgres:5432/ops_service
OPS_LOGGING__LEVEL=info
OPS_LOGGING__FORMAT=json  # 生产环境使用 JSON 格式
OPS_SECURITY__RATE_LIMIT_RPS=100
OPS_SECURITY__JWT_SECRET=<强密码至少32字符>
```

## 生产环境部署

在生产环境中，**不依赖** `.env` 文件，直接设置环境变量：

```bash
# 方式 1: 直接设置环境变量
export OPS_DATABASE__URL="postgresql://..."
export OPS_LOGGING__LEVEL="info"
./ops-system

# 方式 2: 使用 systemd 服务文件
# 在 /etc/systemd/system/ops-system.service 中配置 Environment=...

# 方式 3: 使用 Docker
docker run -e OPS_DATABASE__URL="..." ops-system
```

## 常见问题

### Q: 为什么环境变量使用双下划线？

A: 配置库使用 `__` 作为嵌套字段的分隔符。例如 `OPS_DATABASE__URL` 会被解析为 `database.url`。

### Q: 为什么 `sqlx::query` 报 "dynamic SQL strings should be audited"？

A: sqlx 0.9 起只接受 `'static` SQL 字符串。动态拼接的 SQL（如按筛选条件构造 WHERE 子句）
需人工确认无注入风险后用 `AssertSqlSafe(...)` 包装——只允许拼接固定模板与 `$N` 占位符，
用户输入一律 `.bind()` 参数化。详见 [DEPENDENCY_UPGRADE_HOLD.md](DEPENDENCY_UPGRADE_HOLD.md)。

### Q: 如何运行需要数据库的集成测试？

A: 启动开发数据库后，通过 `TEST_DATABASE_URL` 指定连接串并包含被忽略的用例：

```bash
docker-compose -f docker-compose.dev.yml up -d
TEST_DATABASE_URL=postgresql://ops_user:dev_password@localhost:5432/ops_service_test \
  cargo test -p ops-service -- --include-ignored
```

### Q: 开发环境无法连接数据库？

A: 确保数据库容器正在运行：
```bash
docker ps | grep ops-postgres-dev
docker-compose -f docker-compose.dev.yml ps
```

### Q: 如何不使用 Docker 运行？

A: 设置 `OPS_DATABASE__URL` 连接到系统的 PostgreSQL：
```bash
export OPS_DATABASE__URL="postgresql://user:pass@localhost/ops_db"
cargo run
```

## 环境变量完整列表

参见 [.env.example](.env.example) 文件获取所有可配置项。
