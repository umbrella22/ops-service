# {{BINARY_NAME}} v{{VERSION}} Docker 部署指南

## 目录
- [简介](#简介)
- [前置要求](#前置要求)
- [快速开始](#快速开始)
- [配置说明](#配置说明)
- [服务管理](#服务管理)
- [数据持久化](#数据持久化)
- [网络配置](#网络配置)
- [备份与恢复](#备份与恢复)
- [监控与日志](#监控与日志)
- [故障排除](#故障排除)
- [生产环境建议](#生产环境建议)

## 简介

{{BINARY_NAME}} 提供 Docker 模式部署，所有服务（应用、数据库、反向代理）都运行在独立的容器中，具有以下优势:

- ✅ **环境隔离**: 应用与系统环境完全隔离
- ✅ **一键部署**: 无需手动配置数据库
- ✅ **易于管理**: 统一的容器管理
- ✅ **快速扩展**: 可轻松扩展服务实例
- ✅ **版本升级**: 简单的版本切换和回滚

## 前置要求

### 系统要求
- **操作系统**: Linux (推荐 Ubuntu 20.04+, CentOS 8+, Debian 11+)
- **内存**: 最低 1 GB，推荐 2 GB
- **磁盘**: 最低 2 GB（包含数据库数据）
- **CPU**: 最低 1 核，推荐 2 核

### 软件要求
- Docker 20.10 或更高版本
- Docker Compose 2.0 或更高版本

### 安装 Docker

```bash
# 使用官方脚本安装 Docker
curl -fsSL https://get.docker.com | sh

# 将当前用户添加到 docker 组（可选，避免每次使用 sudo）
sudo usermod -aG docker $USER

# 安装 Docker Compose
# Docker Compose V2 已包含在 Docker 插件中
docker compose version

# 或者安装独立的 Docker Compose V1
sudo curl -L "https://github.com/docker/compose/releases/download/v2.20.0/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
sudo chmod +x /usr/local/bin/docker-compose
```

## 快速开始

### 方法 1: 使用一键初始化脚本（推荐）

```bash
# 1. 解压归档
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-x86_64.tar.gz
cd linux-x86_64

# 2. 运行一键初始化
sudo ./init.sh

# 选择 Docker 模式，脚本会自动:
# - 检测 Docker 环境
# - 安装配置文件到 /etc/{{BINARY_NAME}}/docker
# - 生成随机密码
# - 启动所有容器
```

### 方法 2: 手动安装

```bash
# 1. 解压归档
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-x86_64.tar.gz
cd linux-x86_64

# 2. 运行安装脚本
sudo ./scripts/install.sh --docker --seed-data

# 3. 进入 Docker 目录
cd /etc/{{BINARY_NAME}}/docker

# 4. 启动服务
docker-compose up -d

# 5. 查看日志
docker-compose logs -f
```

## 配置说明

### 环境变量文件

配置文件位置: `/etc/{{BINARY_NAME}}/docker/.env`

```bash
# ================================
# PostgreSQL 数据库配置
# ================================
POSTGRES_DB=ops_service                    # 数据库名称
POSTGRES_USER=ops_user                    # 数据库用户
POSTGRES_PASSWORD=<随机生成的密码>        # 数据库密码（安装时自动生成）

# ================================
# 应用配置
# ================================
LOG_LEVEL=info                            # 日志级别: debug, info, warn, error
ALLOWED_IPS=                              # 允许访问的 IP（空表示允许所有）
ALLOWED_NETWORKS=                         # 允许访问的网络（空表示允许所有）

# ================================
# 种子数据配置
# ================================
LOAD_SEED_DATA=true                       # 是否加载种子数据（演示数据）

# ================================
# 服务端口配置
# ================================
# 应用内部端口（一般不需要修改）
API_PORT=3000
# PostgreSQL 端口（仅容器内部使用）
POSTGRES_PORT=5432
# Nginx HTTP 端口
NGINX_HTTP_PORT=80
# Nginx HTTPS 端口
NGINX_HTTPS_PORT=443
```

### 修改配置

```bash
# 1. 编辑环境变量文件
sudo nano /etc/{{BINARY_NAME}}/docker/.env

# 2. 重启服务使配置生效
cd /etc/{{BINARY_NAME}}/docker
docker-compose restart

# 或者重启特定服务
docker-compose restart api
docker-compose restart nginx
```

### Docker Compose 配置文件

主配置文件: `/etc/{{BINARY_NAME}}/docker/docker-compose.yml`

该文件定义了以下服务:

#### 1. PostgreSQL 数据库服务
```yaml
postgres:
  image: postgres:15-alpine
  environment:
    - POSTGRES_DB=${POSTGRES_DB}
    - POSTGRES_USER=${POSTGRES_USER}
    - POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
  volumes:
    - postgres_data:/var/lib/postgresql/data
  ports:
    - "127.0.0.1:5432:5432"  # 仅监听本地
```

#### 2. API 服务
```yaml
api:
  image: {{BINARY_NAME}}:{{VERSION}}
  depends_on:
    - postgres
  environment:
    - OPS_DATABASE_URL=postgresql://...
  volumes:
    - ./config:/etc/{{BINARY_NAME}}
```

#### 3. Nginx 反向代理
```yaml
nginx:
  image: nginx:alpine
  depends_on:
    - api
  ports:
    - "80:80"
    - "443:443"
  volumes:
    - ./nginx.conf:/etc/nginx/nginx.conf
```

## 服务管理

### 基本命令

```bash
# 进入工作目录
cd /etc/{{BINARY_NAME}}/docker

# 启动所有服务
docker-compose up -d

# 停止所有服务
docker-compose down

# 重启所有服务
docker-compose restart

# 查看服务状态
docker-compose ps

# 查看服务日志
docker-compose logs -f

# 查看特定服务日志
docker-compose logs -f api
docker-compose logs -f postgres
```

### 使用管理脚本

更简单的方式是使用提供的统一管理脚本:

```bash
# 从安装目录运行
cd /path/to/linux-x86_64

# 启动服务
sudo ./scripts/start.sh

# 停止服务
sudo ./scripts/stop.sh

# 重启服务
sudo ./scripts/restart.sh

# 查看状态
sudo ./scripts/status.sh
```

这些脚本会自动检测 Docker 模式并执行相应命令。

### 容器操作

```bash
# 进入运行中的容器
docker-compose exec api sh
docker-compose exec postgres sh

# 在容器中执行命令
docker-compose exec api ps aux
docker-compose exec postgres psql -U ops_user -d ops_service

# 查看容器资源使用
docker stats

# 查看容器详细信息
docker inspect <container_id>
```

## 数据持久化

### Docker 卷

{{BINARY_NAME}} 使用 Docker 卷来持久化数据:

```bash
# 查看所有卷
docker volume ls | grep {{BINARY_NAME}}

# 查看卷详情
docker volume inspect <volume_name>

# 备份卷
docker run --rm -v <volume_name>:/data -v $(pwd):/backup alpine tar czf /backup/backup.tar.gz /data

# 恢复卷
docker run --rm -v <volume_name>:/data -v $(pwd):/backup alpine tar xzf /backup/backup.tar.gz -C /
```

### 数据库数据

PostgreSQL 数据存储在命名卷中:
- 卷名: `postgres_data` 或 `<project>_postgres_data`
- 位置: Docker 管理的卷存储空间

### 应用配置

配置文件存储在宿主机:
- 位置: `/etc/{{BINARY_NAME}}/docker/`
- 包括:
  - `.env` - 环境变量
  - `docker-compose.yml` - Compose 配置
  - `nginx.conf` - Nginx 配置（如果有）

## 网络配置

### 默认网络配置

Docker Compose 自动创建一个桥接网络，所有服务连接到此网络:

```bash
# 查看网络
docker network ls

# 查看网络详情
docker network inspect <network_name>
```

### 端口映射

默认端口映射:

| 服务 | 容器端口 | 宿主机端口 | 说明 |
|------|---------|-----------|------|
| Nginx | 80 | 80 | HTTP |
| Nginx | 443 | 443 | HTTPS |
| PostgreSQL | 5432 | 127.0.0.1:5432 | 数据库（仅本地） |

### 修改端口

如果需要修改端口，编辑 `docker-compose.yml`:

```yaml
services:
  nginx:
    ports:
      - "8080:80"    # 将 HTTP 改为 8080
      - "8443:443"   # 将 HTTPS 改为 8443

  postgres:
    ports:
      - "127.0.0.1:5433:5432"  # 将 PostgreSQL 改为 5433
```

### 反向代理配置

Nginx 作为反向代理，配置文件位于 `/etc/{{BINARY_NAME}}/docker/nginx.conf`:

```nginx
upstream api_backend {
    server api:3000;
}

server {
    listen 80;
    server_name localhost;

    location / {
        proxy_pass http://api_backend;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

## 备份与恢复

### 使用备份脚本

```bash
# 从安装目录运行
cd /path/to/linux-x86_64

# 创建备份
sudo ./scripts/backup.sh

# 备份包括:
# - Docker 配置文件
# - 数据库转储（如果容器正在运行）
# - 备份位置: /var/backups/{{BINARY_NAME}}/
```

### 手动备份

#### 1. 备份配置文件

```bash
# 备份整个 docker 目录
sudo tar czf {{BINARY_NAME}}-config-$(date +%Y%m%d).tar.gz /etc/{{BINARY_NAME}}/docker
```

#### 2. 备份数据库

```bash
cd /etc/{{BINARY_NAME}}/docker

# 从运行中的容器备份数据库
docker-compose exec postgres pg_dump -U ops_user ops_service > backup.sql

# 或使用 docker exec
docker exec {{BINARY_NAME}}-postgres-1 pg_dump -U ops_user ops_service > backup.sql
```

#### 3. 备份 Docker 卷

```bash
# 备份 PostgreSQL 数据卷
docker run --rm \
  -v postgres_data:/data \
  -v $(pwd):/backup \
  alpine tar czf /backup/postgres-data-$(date +%Y%m%d).tar.gz /data
```

### 恢复数据

#### 恢复数据库

```bash
cd /etc/{{BINARY_NAME}}/docker

# 恢复数据库
cat backup.sql | docker-compose exec -T postgres psql -U ops_user ops_service
```

#### 恢复配置

```bash
# 停止服务
docker-compose down

# 恢复配置文件
sudo tar xzf {{BINARY_NAME}}-config-20240101.tar.gz -C /

# 重启服务
docker-compose up -d
```

## 监控与日志

### 查看日志

```bash
cd /etc/{{BINARY_NAME}}/docker

# 查看所有服务日志
docker-compose logs

# 实时跟踪日志
docker-compose logs -f

# 查看最近 100 行日志
docker-compose logs --tail=100

# 查看特定服务日志
docker-compose logs -f api
docker-compose logs -f postgres
docker-compose logs -f nginx

# 查看特定时间范围的日志
docker-compose logs --since 2024-01-01T00:00:00
docker-compose logs --until 2024-01-02T00:00:00
```

### 日志管理

配置日志轮转（编辑 `/etc/{{BINARY_NAME}}/docker/docker-compose.yml`）:

```yaml
services:
  api:
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
```

### 容器监控

```bash
# 查看容器资源使用
docker stats

# 查看特定容器
docker stats {{BINARY_NAME}}-api-1

# 查看容器进程
docker top {{BINARY_NAME}}-api-1
```

### 健康检查

```bash
# 检查 API 健康状态
curl http://localhost:3000/api/health

# 检查就绪状态
curl http://localhost:3000/api/ready

# 查看容器健康状态
docker-compose ps
```

## 故障排除

### 容器无法启动

#### 1. 检查端口占用

```bash
# 查看端口占用
sudo netstat -tulpn | grep :80
sudo netstat -tulpn | grep :443

# 或使用 lsof
sudo lsof -i :80
sudo lsof -i :443
```

#### 2. 检查日志

```bash
docker-compose logs
docker-compose logs api
docker-compose logs postgres
```

#### 3. 验证配置

```bash
# 检查环境变量文件
cat /etc/{{BINARY_NAME}}/docker/.env

# 验证 Docker Compose 配置
docker-compose config

# 检查网络
docker network ls
docker network inspect <network_name>
```

### 数据库连接问题

```bash
# 1. 检查 PostgreSQL 容器是否运行
docker-compose ps postgres

# 2. 进入数据库容器
docker-compose exec postgres sh

# 3. 测试数据库连接
docker-compose exec postgres psql -U ops_user -d ops_service

# 4. 检查数据库日志
docker-compose logs postgres
```

### 性能问题

```bash
# 查看容器资源使用
docker stats

# 查看容器详细信息
docker inspect <container_id>

# 限制资源使用（在 docker-compose.yml 中配置）
services:
  api:
    deploy:
      resources:
        limits:
          cpus: '1'
          memory: 512M
        reservations:
          cpus: '0.5'
          memory: 256M
```

### 磁盘空间不足

```bash
# 查看 Docker 占用空间
docker system df

# 清理未使用的镜像
docker image prune

# 清理未使用的容器
docker container prune

# 清理未使用的卷
docker volume prune

# 完全清理（谨慎使用）
docker system prune -a --volumes
```

## 生产环境建议

### 1. 安全配置

```bash
# 使用强密码
POSTGRES_PASSWORD=$(openssl rand -base64 32)

# 限制数据库端口仅本地访问
ports:
  - "127.0.0.1:5432:5432"

# 配置防火墙
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw enable
```

### 2. SSL/TLS 配置

```bash
# 使用 Let's Encrypt 获取免费证书
sudo apt install certbot

# 获取证书
sudo certbot certonly --standalone -d your-domain.com

# 修改 Nginx 配置使用 HTTPS
# /etc/{{BINARY_NAME}}/docker/nginx.conf
```

### 3. 资源限制

在 `docker-compose.yml` 中配置资源限制:

```yaml
services:
  api:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 1G
        reservations:
          cpus: '1'
          memory: 512M

  postgres:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '1'
          memory: 1G
```

### 4. 日志管理

```yaml
services:
  api:
    logging:
      driver: "json-file"
      options:
        max-size: "50m"
        max-file: "5"

  postgres:
    logging:
      driver: "json-file"
      options:
        max-size: "100m"
        max-file: "5"
```

### 5. 自动重启策略

```yaml
services:
  api:
    restart: always

  postgres:
    restart: always

  nginx:
    restart: always
```

### 6. 健康检查

```yaml
services:
  api:
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/api/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  postgres:
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ops_user"]
      interval: 10s
      timeout: 5s
      retries: 5
```

### 7. 定期备份

设置 cron 任务定期备份:

```bash
# 编辑 crontab
sudo crontab -e

# 添加每日凌晨 2 点备份
0 2 * * * cd /etc/{{BINARY_NAME}}/docker && /path/to/backup.sh
```

### 8. 监控和告警

考虑使用:
- Prometheus + Grafana 监控容器
- Alertmanager 配置告警
- ELK Stack 收集和分析日志
- Sentry 错误跟踪

### 9. 更新策略

```bash
# 1. 备份当前版本
sudo ./scripts/backup.sh

# 2. 拉取新镜像
docker-compose pull

# 3. 重启服务
docker-compose up -d

# 4. 清理旧镜像
docker image prune
```

## 更多资源

- [部署指南](DEPLOY_CN.md)
- [安全配置指南](SECURITY_CN.md)
- [故障排除指南](TROUBLESHOOTING_CN.md)
- [升级指南](UPGRADE_CN.md)

## 获取帮助

如果遇到问题:

1. 查看日志: `docker-compose logs -f`
2. 检查状态: `docker-compose ps`
3. 查看文档: [https://docs.example.com](https://docs.example.com)
4. 提交问题: [GitHub Issues](https://github.com/example/issues)
