# 运维系统 (Ops System)

一个现代化的企业级运维管理平台，基于 Rust 构建，提供资产管理、用户认证、权限控制和审计日志等核心功能。

## ✨ 特性

- 🏗️ **现代化架构** - 基于 Rust + Axum + PostgreSQL
- 🔐 **安全认证** - JWT 令牌、密码哈希、API Key 支持
- 👥 **权限管理** - 基于角色的访问控制 (RBAC)
- 📊 **资产管理** - 服务器、网络设备、服务实例管理
- 📝 **审计日志** - 完整的操作审计和追踪
- 🎯 **高性能** - 异步 I/O，连接池优化
- 🐳 **容器就绪** - Docker 支持，Kubernetes 友好
- ✅ **测试完善** - 53个自动化测试，CI/CD 就绪

## 📋 目录

- [快速开始 (Docker)](#快速开始-docker)
- [快速开始 (Native)](#快速开始-native)
- [项目结构](#项目结构)
- [功能模块](#功能模块)
- [开发指南](#开发指南)
- [测试](#测试)
- [构建与打包](#构建与打包)
- [部署](#部署)
- [贡献](#贡献)

## 🚀 快速开始 (Docker)

**推荐使用 Docker 部署**，这是最简单、最可靠的部署方式。所有依赖（数据库、应用、Nginx）都已打包好，开箱即用。

### 前置要求

- Docker 20.10+
- Docker Compose 2.0+ (或 Docker Compose V2)

### 1. 克隆仓库

```bash
git clone <https://github.com/umbrella22/ops-service>
cd ops-service
```

### 2. 启动服务

```bash
# 使用 Docker Compose 启动所有服务
docker-compose up -d

# 查看日志
docker-compose logs -f

# 检查服务状态
docker-compose ps
```

服务将在以下端口启动：
- **HTTP**: http://localhost:80
- **HTTPS**: https://localhost:443

### 3. 验证安装

```bash
# 健康检查
curl http://localhost/health

# 就绪检查
curl http://localhost/ready
```

### 架构说明

Docker 部署采用微服务架构，包含以下独立容器：

1. **PostgreSQL 数据库** - 官方 postgres:16 镜像，仅监听 localhost
2. **API 服务** - 基于 Debian 的最小化镜像，内网运行
3. **Nginx 反向代理** - 提供 HTTPS 和静态文件服务

所有服务通过内部网络通信，数据库不对外暴露，确保安全。

### 管理命令

```bash
# 停止服务
docker-compose down

# 重启服务
docker-compose restart

# 查看日志
docker-compose logs -f [service_name]

# 进入容器
docker-compose exec api bash

# 更新服务
docker-compose pull
docker-compose up -d
```

## 🔧 快速开始 (Native)

如果您希望直接在系统上运行服务（不使用 Docker），可以按照以下步骤操作。

### 前置要求

- Rust 1.75+
- PostgreSQL 12+

### 1. 克隆仓库

```bash
git clone <https://github.com/umbrella22/ops-service>
cd ops-service
```

### 2. 配置数据库

```bash
# 创建数据库
createdb ops_system

# 设置环境变量
export OPS_DATABASE__URL="postgresql://postgres:postgres@localhost:5432/ops_system"
export OPS_SECURITY__JWT_SECRET="your-secret-key-min-32-characters-long"
```

### 3. 运行迁移

```bash
# 安装 sqlx-cli
cargo install sqlx-cli --no-default-features --features rustls,postgres

# 运行迁移
sqlx migrate run --database-url $OPS_DATABASE__URL
```

### 4. 运行服务

```bash
# 开发模式
cargo run

# 生产模式
cargo build --release
./target/release/ops-system
```

服务将在 `http://localhost:3000` 启动。

### 5. 验证安装

```bash
# 健康检查
curl http://localhost:3000/health

# 就绪检查
curl http://localhost:3000/ready
```

## 📁 项目结构

```
运维系统/
├── src/                    # 源代码
│   ├── main.rs             # 程序入口
│   ├── lib.rs              # 库入口
│   ├── config.rs           # 配置管理
│   ├── db.rs               # 数据库连接
│   ├── error.rs            # 错误处理
│   ├── telemetry.rs        # 日志与指标
│   ├── middleware.rs       # HTTP 中间件
│   ├── routes.rs           # 路由注册
│   ├── auth/               # 认证模块
│   │   ├── jwt.rs          # JWT 令牌
│   │   ├── password.rs     # 密码哈希
│   │   ├── api_key.rs      # API Key 管理
│   │   └── middleware.rs   # 认证中间件
│   ├── models/             # 数据模型
│   ├── handlers/           # HTTP 处理器
│   ├── repository/         # 数据访问层
│   └── services/           # 业务逻辑层
├── migrations/             # 数据库迁移
├── tests/                  # 集成测试
├── scripts/                # 工具脚本
│   ├── ci_test.sh          # CI 测试脚本
│   ├── test_local.sh       # 本地测试脚本
│   └── test_docker.sh      # Docker 测试脚本
├── Dockerfile              # Docker 镜像
├── docker-compose.yml      # Docker Compose
└── Cargo.toml              # 项目配置
```

## 🎯 功能模块

### 认证与授权

- **JWT 令牌** - Access Token + Refresh Token
- **密码哈希** - Argon2 加密
- **API Key** - 服务账户支持
- **多因素认证** - 可扩展的 MFA 架构

### 用户管理

- 用户 CRUD 操作
- 角色分配
- 权限继承
- 登录历史追踪

### 资产管理

- 资产组管理
- 主机管理
- 环境隔离（开发/测试/生产）
- 标签分类

### 审计日志

- 操作审计
- 登录日志
- 数据变更追踪
- Trace ID 关联

## 🔧 开发指南

### 环境配置

通过环境变量配置应用：

```bash
# 服务器配置
export OPS_SERVER__ADDR="0.0.0.0:3000"

# 数据库配置
export OPS_DATABASE__URL="postgresql://user:pass@localhost:5432/db"
export OPS_DATABASE__MAX_CONNECTIONS=10

# 安全配置
export OPS_SECURITY__JWT_SECRET="your-secret-key-min-32-characters"
export OPS_SECURITY__ACCESS_TOKEN_EXP_SECS=900
export OPS_SECURITY__REFRESH_TOKEN_EXP_SECS=604800

# 日志配置
export OPS_LOGGING__LEVEL="info"
export OPS_LOGGING__FORMAT="json"
```

### 代码规范

```bash
# 格式化代码
cargo fmt

# 代码检查
cargo clippy -- -D warnings

# 运行测试
cargo test
```

### 构建优化

```bash
# 开发构建
cargo build

# 发布构建
cargo build --release

# 查看构建大小
ls -lh target/release/ops-system
```

## 🧪 测试

### 运行测试

```bash
# 运行所有测试
make test-all
# 或
cargo test -- --test-threads=1

# 运行单元测试
make test-unit

# 运行集成测试
make test-integration

# 生成覆盖率报告
make coverage
```

### 测试覆盖

- **单元测试**: 26 个
- **集成测试**: 27 个
- **总计**: 53 个测试

详细测试文档: [tests/README.md](tests/README.md)

## 📦 构建与打包

项目提供了完整的构建和打包系统，支持多平台分发。

### 快速打包

```bash
# 为当前平台创建包 (Linux x86_64)
make package

# 为所有平台创建包
make package-all

# 创建发布归档 (.tar.gz)
make dist-all
```

### 可用的构建命令

| 命令 | 说明 |
|------|------|
| `make package` | 创建当前平台包 |
| `make package-x86_64` | 创建 Linux x86_64 包 |
| `make package-arm64` | 创建 Linux ARM64 包 |
| `make package-all` | 创建所有平台包 |
| `make dist-all` | 创建所有平台的发布归档 |
| `make package-validate` | 验证包内容 |
| `make package-clean` | 清理构建目录 |

### 构建产物

构建产物将存放在 `build/` 目录中：

```
build/
├── linux-x86_64/              # x86_64 平台包
│   ├── bin/ops-system        # 可执行文件
│   ├── migrations/            # 数据库迁移文件
│   ├── config/.env.example    # 配置模板
│   ├── docker/                # Docker 配置
│   ├── nginx/                 # Nginx 配置
│   ├── scripts/               # 管理脚本
│   ├── systemd/               # Systemd 服务文件
│   ├── docs/                  # 部署文档
│   └── VERSION, CHECKSUM, BUILD_INFO.txt
└── dist/                      # 发布归档
    └── ops-system-0.1.0-linux-x86_64.tar.gz
```

### 交叉编译

项目支持为 ARM64 平台交叉编译。

#### 设置 ARM64 交叉编译环境

```bash
# 1. 添加 ARM64 目标
rustup target add aarch64-unknown-linux-gnu

# 2. 安装交叉编译工具链 (Ubuntu/Debian)
sudo apt install gcc-aarch64-linux-gnu

# 3. 构建 ARM64 包
make package-arm64
```

### 管理脚本

每个构建包包含以下管理脚本：

- `install.sh` - 安装脚本
- `start.sh` - 启动服务
- `stop.sh` - 停止服务
- `restart.sh` - 重启服务
- `status.sh` - 查看状态
- `update.sh` - 更新版本
- `backup.sh` - 备份数据
- `uninstall.sh` - 卸载程序

### 使用打包的发布版本

```bash
# 1. 解压发布包
tar -xzf build/dist/ops-system-0.1.0-linux-x86_64.tar.gz
cd linux-x86_64

# 2. 运行安装脚本
sudo ./scripts/install.sh

# 3. 配置环境变量
sudo nano /etc/ops-system/env

# 4. 启动服务
sudo systemctl start ops-system

# 5. 查看状态
sudo ./scripts/status.sh
```

## 🐳 部署指南

### Docker 部署（推荐）

使用发布包安装：

```bash
# 1. 下载并解压发布包
tar -xzf ops-system-0.1.0-linux-x86_64.tar.gz
cd linux-x86_64

# 2. 运行安装脚本（默认 Docker 模式）
sudo ./scripts/install.sh

# 3. 启动服务
cd /etc/ops-system/docker
docker-compose up -d
```

如需使用 Native 模式：

```bash
# 使用 --native 参数
sudo ./scripts/install.sh --native

# 或
sudo INSTALL_MODE=native ./scripts/install.sh
```

### 手动 Docker 部署

```bash
# 构建镜像
docker build -t ops-system:latest .

# 运行容器
docker run -d \
  --name ops-system \
  -p 3000:3000 \
  -e OPS_DATABASE__URL="postgresql://..." \
  -e OPS_SECURITY__JWT_SECRET="..." \
  ops-system:latest
```

### Kubernetes 部署

```bash
# 创建 ConfigMap
kubectl apply -f k8s/configmap.yaml

# 创建 Secret
kubectl apply -f k8s/secret.yaml

# 部署
kubectl apply -f k8s/deployment.yaml
kubectl apply -f k8s/service.yaml
```

### 反向代理配置

**Nginx 示例**:

```nginx
location / {
    proxy_pass http://localhost:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

## 📚 API 文档

### 健康检查端点

- `GET /health` - 存活探针
- `GET /ready` - 就绪探针
- `GET /metrics` - Prometheus 指标

### 认证端点

- `POST /api/v1/auth/login` - 用户登录
- `POST /api/v1/auth/refresh` - 刷新令牌
- `POST /api/v1/auth/logout` - 用户登出
- `GET /api/v1/auth/me` - 获取当前用户

### 资产端点

- `GET /api/v1/groups` - 列出资产组
- `POST /api/v1/groups` - 创建资产组
- `GET /api/v1/groups/:id` - 获取资产组详情
- `PUT /api/v1/groups/:id` - 更新资产组
- `DELETE /api/v1/groups/:id` - 删除资产组

### 主机端点

- `GET /api/v1/hosts` - 列出主机
- `POST /api/v1/hosts` - 创建主机
- `GET /api/v1/hosts/:id` - 获取主机详情
- `PUT /api/v1/hosts/:id` - 更新主机
- `DELETE /api/v1/hosts/:id` - 删除主机

## 🔒 安全建议

1. **密钥管理**
   - 使用环境变量或密钥管理服务存储敏感信息
   - 定期轮换 JWT 密钥
   - 生产环境使用强密码

2. **网络安全**
   - 启用 HTTPS
   - 配置防火墙规则
   - 使用 IP 白名单

3. **访问控制**
   - 遵循最小权限原则
   - 定期审计用户权限
   - 启用 MFA（未来）

## 🤝 贡献

欢迎贡献！

## 📄 许可证

MIT License

## 支持

- 🐛 [问题反馈](https://github.com/umbrella22/ops-service/issues)

## 🙏 致谢

感谢所有贡献者！
