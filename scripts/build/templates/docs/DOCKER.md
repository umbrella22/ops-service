# Docker 部署指南

本目录包含用于构建 Docker 镜像的文件，基于已编译的二进制文件。

## 目录结构

```
docker/
├── Dockerfile    # Docker 镜像定义文件
├── build.sh      # 多架构镜像构建脚本
└── README.md     # 本文档
```

## 特性

- ✅ 支持多架构（x86_64/amd64 和 ARM64）
- ✅ 非 root 用户运行
- ✅ 健康检查支持
- ✅ 最小化镜像体积
- ✅ 自动架构检测

## 快速开始

### 1. 构建镜像

```bash
# 进入对应架构的 docker 目录
cd build/linux-x86_64/docker  # 或 build/linux-arm64/docker

# 构建镜像
./build.sh
```

### 2. 运行容器

```bash
# 基础运行（不启动 Runner）
docker run -d \
  --name {{BINARY_NAME}} \
  -p 3000:3000 \
  -e DATABASE_URL="postgresql://user:pass@host:5432/dbname" \
  {{BINARY_NAME}}:latest-amd64

# 挂载配置文件
docker run -d \
  --name {{BINARY_NAME}} \
  -p 3000:3000 \
  -v /path/to/config.toml:/app/config/config.toml \
  -e DATABASE_URL="postgresql://user:pass@host:5432/dbname" \
  {{BINARY_NAME}}:latest-amd64

# 运行并支持启动 Runner 容器（需要挂载 docker.sock）
docker run -d \
  --name {{BINARY_NAME}} \
  -p 3000:3000 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -e DATABASE_URL="postgresql://user:pass@host:5432/dbname" \
  {{BINARY_NAME}}:latest-amd64
```

### 3. 环境变量

| 变量名 | 说明 | 必填 |
|--------|------|------|
| `DATABASE_URL` | PostgreSQL 数据库连接字符串 | 是 |
| `RUST_LOG` | 日志级别 (info/debug/trace) | 否 |
| `BIND_ADDRESS` | 绑定地址 (默认: 0.0.0.0:3000) | 否 |

## 高级用法

### 自定义镜像名称和版本

```bash
IMAGE_NAME=my-app VERSION=1.0.0 ./build.sh
```

### 构建并推送到私有仓库

```bash
REGISTRY=registry.example.com/ ./build.sh --push
```

### 使用 Docker Compose

创建 `docker-compose.yml`:

```yaml
version: '3.8'

services:
  {{BINARY_NAME}}:
    image: {{BINARY_NAME}}:latest-amd64
    container_name: {{BINARY_NAME}}
    ports:
      - "3000:3000"
    volumes:
      # 挂载 Docker Socket 以支持 Runner 功能
      - /var/run/docker.sock:/var/run/docker.sock
    environment:
      - DATABASE_URL=postgresql://postgres:password@db:5432/{{BINARY_NAME}}
      - RUST_LOG=info
    depends_on:
      - db
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 3s
      retries: 3

  db:
    image: postgres:15-bookworm
    container_name: {{BINARY_NAME}}-db
    environment:
      - POSTGRES_DB={{BINARY_NAME}}
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      - postgres_data:/var/lib/postgresql/data
    restart: unless-stopped

volumes:
  postgres_data:
```

**注意**：如果不使用 Runner 功能，可以移除 `volumes` 中的 docker.sock 挂载。

启动服务：

```bash
docker-compose up -d
```

## 多架构支持

本项目支持两种架构：

| 架构 | Docker Platform | 目录名 |
|------|-----------------|--------|
| x86_64 | linux/amd64 | `linux-x86_64/` |
| ARM64 | linux/arm64 | `linux-arm64/` |

构建脚本会根据所在目录自动选择正确的架构。

## 镜像构建说明

本 Dockerfile **不包含源码编译**，仅将预编译的二进制文件打包到镜像中。这种方式具有以下优势：

- ⚡ 更快的构建速度
- 📦 更小的镜像体积（无需构建工具链）
- 🔒 更好的安全性（构建工具不会进入生产镜像）
- 🎯 一致的构建结果

## Runner 容器管理架构

{{BINARY_NAME}} 支持在宿主机上启动和管理 Runner 容器来执行任务。架构如下：

```
┌─────────────────────────────────────────────────────────┐
│  宿主机 (Host Machine)                                  │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ Docker 容器: {{BINARY_NAME}}                    │   │
│  │ - 管理 API (端口 3000)                          │   │
│  │ - 任务调度和监控                                │   │
│  └─────────────────────────────────────────────────┘   │
│                      ▼                                  │
│              启动 Runner 容器                            │
│                      ▼                                  │
│  ┌─────────────────────────────────────────────────┐   │
│  │ Docker 容器: runner-{task-id}                   │   │
│  │ - 执行具体任务                                  │   │
│  │ - 完成后自动销毁                                │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  Docker Daemon (docker.sock)                           │
└─────────────────────────────────────────────────────────┘
```

### 如何启用 Runner 功能

要让 {{BINARY_NAME}} 能够启动 Runner 容器，需要：

1. **挂载 Docker Socket**：
   ```bash
   -v /var/run/docker.sock:/var/run/docker.sock
   ```

2. **容器已安装 Docker CLI**：
   - 镜像已包含 Docker CLI 客户端
   - 用户已添加到 docker 组

### 安全建议

- ✅ **推荐**：仅在内网环境或受信任的网络中使用
- ⚠️ **注意**：挂载 docker.sock 会赋予容器对宿主机 Docker 的完全控制权
- 🔒 **加固**：考虑使用 AppArmor/SELinux 限制容器权限
- 📊 **监控**：监控容器的 Docker API 调用和资源使用

## Runner 安全最佳实践

根据自建 Runner 的安全要求，以下是关键的安全配置：

### 1. 工作目录隔离

Runner 容器必须在固定的工作目录下运行，防止误删宿主机文件：

```yaml
environment:
  # 设置 Runner 工作目录前缀
  - RUNNER_WORK_DIR=/tmp/{{BINARY_NAME}}-workspace
  - RUNNER_WORKSPACE_PREFIX=/tmp/{{BINARY_NAME}}-workspace/
```

清理策略只会删除此前缀下的目录，并且会进行强校验。

### 2. 资源限制

限制容器资源使用，防止任务耗尽宿主机资源：

```yaml
deploy:
  resources:
    limits:
      cpus: '2'
      memory: 2G
    reservations:
      cpus: '0.5'
      memory: 512M
```

### 3. 权限限制

使用只读文件系统和安全选项：

```yaml
# 禁用特权提升
security_opt:
  - no-new-privileges:true

# 可选：只读根文件系统
# read_only: true
# tmpfs:
#   - /tmp:rw,size=100M
```

### 4. Docker Socket 只读挂载

如果不需要通过 docker exec 等命令操作容器，可以以只读方式挂载：

```yaml
volumes:
  - /var/run/docker.sock:/var/run/docker.sock:ro
```

### 5. 使用 AppArmor/SELinux

查看 `docker-compose.secure.yml` 和 `../security/AppArmor.profile` 获取完整的安全配置示例。

```bash
# 加载 AppArmor 配置
sudo cp security/AppArmor.profile /etc/apparmor.d/docker-{{BINARY_NAME}}
sudo apparmor_parser -r /etc/apparmor.d/docker-{{BINARY_NAME}}

# 在 docker-compose.yml 中启用
security_opt:
  - apparmor:docker-{{BINARY_NAME}}
```

### 6. 网络隔离

使用独立的 Docker 网络，限制 Runner 容器的网络访问：

```yaml
environment:
  - DOCKER_NETWORK={{BINARY_NAME}}-network

networks:
  {{BINARY_NAME}}-network:
    driver: bridge
    ipam:
      config:
        - subnet: 172.28.0.0/16
```

### 7. 审计和监控

启用 Docker API 审计日志：

```bash
# 在宿主机上启用 Docker 审计
sudo auditctl -w /var/run/docker.sock -p wa -k docker
```

### 8. 避免高危能力

**禁止**以下操作：
- ❌ 不要使用 `--privileged` 标志
- ❌ 不要添加 `CAP_SYS_ADMIN`、`CAP_SYS_MODULE` 等高危能力
- ❌ 不要挂载宿主机根目录
- ❌ 不要允许 Runner 访问敏感路径（如 `/root`、`/var/log` 等）

### Runner 网络配置

Runner 容器需要能够访问数据库和其他服务。有两种网络配置方式：

#### 方式 1：使用 host 网络（推荐用于开发）
```yaml
services:
  {{BINARY_NAME}}:
    network_mode: host
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
```
Runner 容器可以直接访问宿主机的网络栈。

#### 方式 2：使用自定义网络（推荐用于生产）
```yaml
services:
  {{BINARY_NAME}}:
    networks:
      - ops-network
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    environment:
      - DOCKER_NETWORK=ops-network

networks:
  ops-network:
    driver: bridge
```
{{BINARY_NAME}} 会将 Runner 容器加入到指定的 Docker 网络中。

## 健康检查

容器内置健康检查，每 30 秒检查一次：

```bash
curl -f http://localhost:3000/health
```

可以通过 `docker ps` 查看容器健康状态。

## 安全建议

1. **不要在镜像中硬编码敏感信息** - 使用环境变量或 secrets
2. **定期更新基础镜像** - 安全漏洞修复
3. **使用非 root 用户** - 镜像已配置 `opsuser` 用户
4. **限制容器权限** - 使用 `--read-only` 和 `--tmpfs` 挂载

```bash
docker run -d \
  --name {{BINARY_NAME}} \
  --read-only \
  --tmpfs /tmp \
  -p 3000:3000 \
  -e DATABASE_URL="..." \
  {{BINARY_NAME}}:latest-amd64
```

## 故障排查

### 查看容器日志

```bash
docker logs {{BINARY_NAME}}
docker logs -f {{BINARY_NAME}}  # 实时查看
```

### 进入容器调试

```bash
docker exec -it {{BINARY_NAME}} sh
```

### 检查健康状态

```bash
docker inspect {{BINARY_NAME}} --format='{{.State.Health.Status}}'
```

## 生产环境检查清单

- [ ] 设置适当的资源限制（`--memory`, `--cpus`）
- [ ] 配置日志轮转
- [ ] 使用 secrets 管理敏感信息
- [ ] 配置重启策略（`restart: unless-stopped`）
- [ ] 设置监控和告警
- [ ] 备份数据库
- [ ] 测试灾难恢复流程
