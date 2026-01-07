# {{BINARY_NAME}} v{{VERSION}} 部署指南

## 目录
- [系统要求](#系统要求)
- [快速开始](#快速开始)
- [安装模式](#安装模式)
- [安装步骤](#安装步骤)
- [配置](#配置)
- [服务管理](#服务管理)
- [验证安装](#验证安装)
- [故障排除](#故障排除)
- [升级](#升级)
- [卸载](#卸载)

## 系统要求

### 最低配置
- **操作系统**: Linux (x86_64 或 ARM64)
- **内存**: 最低 512 MB，推荐 1 GB
- **磁盘**: 二进制文件 100 MB + 数据库空间
- **CPU**: 最低 1 核

### 软件依赖

#### Native 模式
- PostgreSQL 12+
- systemd (用于服务管理)

#### Docker 模式（推荐）
- Docker 20.10+
- Docker Compose 2.0+

### 可选依赖
- Nginx (用于反向代理)
- curl (用于健康检查)

## 安装模式

{{BINARY_NAME}} 支持两种安装模式:

### 1. Docker 模式 (推荐)

**优点:**
- ✅ 所有服务运行在容器中，环境隔离
- ✅ 更易于管理和升级
- ✅ 不需要手动配置 PostgreSQL
- ✅ 一键启动和停止

**缺点:**
- 需要 Docker 和 Docker Compose
- 占用稍多的系统资源

**适用场景:**
- 生产环境部署
- 快速测试和开发
- 不想手动管理数据库

### 2. Native 模式

**优点:**
- 直接运行为 systemd 服务
- 资源占用较少
- 可以使用系统的 PostgreSQL

**缺点:**
- 需要手动安装和配置 PostgreSQL
- 配置相对复杂

**适用场景:**
- 已有 PostgreSQL 服务器的环境
- 需要与系统服务深度集成
- 资源受限的环境

## 快速开始

### 使用一键初始化脚本（推荐）

```bash
# 1. 解压归档文件
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 2. 运行一键初始化脚本
sudo ./init.sh
```

初始化脚本会:
1. 检测系统是否安装 Docker
2. 询问您选择安装模式（如果检测到 Docker）
3. 询问是否加载种子数据（演示用户和示例数据）
4. 自动完成安装和启动

### 手动安装

如果您需要更多控制，可以手动运行安装脚本:

```bash
# Docker 模式安装
sudo ./scripts/install.sh --docker --seed-data

# Native 模式安装
sudo ./scripts/install.sh --native --seed-data

# 不加载种子数据
sudo ./scripts/install.sh --docker --no-seed-data
```

## 安装步骤

### Docker 模式详细步骤

```bash
# 1. 解压并进入目录
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 2. 运行安装脚本
sudo ./scripts/install.sh --docker

# 3. 查看生成的配置
cat /etc/{{BINARY_NAME}}/docker/.env

# 4. 启动服务
cd /etc/{{BINARY_NAME}}/docker
docker-compose up -d

# 5. 查看日志
docker-compose logs -f
```

### Native 模式详细步骤

```bash
# 1. 安装 PostgreSQL
sudo apt install postgresql postgresql-contrib  # Ubuntu/Debian
# 或
sudo yum install postgresql-server postgresql  # RHEL/CentOS

# 2. 启动 PostgreSQL
sudo systemctl start postgresql
sudo systemctl enable postgresql

# 3. 创建数据库
sudo -u postgres psql
CREATE DATABASE ops_service;
CREATE USER ops_user WITH ENCRYPTED PASSWORD 'your_password';
GRANT ALL PRIVILEGES ON DATABASE ops_service TO ops_user;
\q

# 4. 解压并安装
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 5. 运行安装脚本
sudo ./scripts/install.sh --native

# 6. 编辑配置文件
sudo nano /etc/{{BINARY_NAME}}/env
# 设置数据库 URL: OPS_DATABASE__URL=postgresql://ops_user:your_password@127.0.0.1:5432/ops_service

# 7. 启动服务
sudo ./scripts/start.sh
```

## 配置

### Docker 模式配置

配置文件位置: `/etc/{{BINARY_NAME}}/docker/.env`

```bash
# PostgreSQL 配置
POSTGRES_DB=ops_service
POSTGRES_USER=ops_user
POSTGRES_PASSWORD=<自动生成的随机密码>

# 应用配置
LOG_LEVEL=info
ALLOWED_IPS=

# 种子数据配置
LOAD_SEED_DATA=true  # true 或 false
```

修改配置后重启容器:
```bash
cd /etc/{{BINARY_NAME}}/docker
docker-compose restart
```

### Native 模式配置

配置文件位置: `/etc/{{BINARY_NAME}}/env`

```bash
# 数据库配置 (注意: 使用双下划线 __ 表示嵌套字段)
OPS_DATABASE__URL=postgresql://user:password@127.0.0.1:5432/ops_service
OPS_DATABASE__MAX_CONNECTIONS=10
OPS_DATABASE__MIN_CONNECTIONS=2

# 服务器配置
OPS_SERVER__ADDR=0.0.0.0:3000

# 安全配置
OPS_SECURITY__JWT_SECRET=your-random-secret-key-min-32-chars
OPS_SECURITY__RATE_LIMIT_RPS=100

# 日志配置
OPS_LOGGING__LEVEL=info
```

修改配置后重启服务:
```bash
sudo systemctl restart {{BINARY_NAME}}
```

## 服务管理

### 使用管理脚本（推荐）

所有脚本自动检测安装模式并相应操作:

```bash
# 启动服务
sudo ./scripts/start.sh

# 停止服务
sudo ./scripts/stop.sh

# 重启服务
sudo ./scripts/restart.sh

# 查看状态
sudo ./scripts/status.sh

# 备份数据
sudo ./scripts/backup.sh

# 更新版本
sudo ./scripts/update.sh

# 卸载
sudo ./scripts/uninstall.sh
```

### Docker 模式特定命令

```bash
# 进入 Docker 目录
cd /etc/{{BINARY_NAME}}/docker

# 查看日志
docker-compose logs -f

# 查看容器状态
docker-compose ps

# 重启特定服务
docker-compose restart api

# 停止所有服务
docker-compose down

# 启动所有服务
docker-compose up -d
```

### Native 模式特定命令

```bash
# 启用开机自启
sudo systemctl enable {{BINARY_NAME}}

# 禁用开机自启
sudo systemctl disable {{BINARY_NAME}}

# 查看实时日志
sudo journalctl -u {{BINARY_NAME}} -f

# 查看最近日志
sudo journalctl -u {{BINARY_NAME}} -n 100
```

## 验证安装

### 健康检查

```bash
# 使用 curl
curl http://localhost:3000/api/health

# 预期输出
{"status":"ok"}
```

### 就绪检查

```bash
curl http://localhost:3000/api/ready

# 如果数据库已连接
{"status":"ready"}
```

### 检查服务状态

```bash
# Docker 模式
cd /etc/{{BINARY_NAME}}/docker && docker-compose ps

# Native 模式
sudo systemctl status {{BINARY_NAME}}
```

## 故障排除

### 服务无法启动

1. **检查日志:**
   ```bash
   # Docker 模式
   cd /etc/{{BINARY_NAME}}/docker && docker-compose logs

   # Native 模式
   sudo journalctl -u {{BINARY_NAME}} -n 100
   ```

2. **验证配置:**
   ```bash
   # Docker 模式
   cat /etc/{{BINARY_NAME}}/docker/.env

   # Native 模式
   cat /etc/{{BINARY_NAME}}/env
   ```

3. **检查数据库连接:**
   ```bash
   # Docker 模式
   docker exec -it <postgres_container> psql -U ops_user -d ops_service

   # Native 模式
   psql "postgresql://user:pass@127.0.0.1:5432/ops_service"
   ```

### 权限问题

```bash
# 修复权限
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/lib/{{BINARY_NAME}}
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/log/{{BINARY_NAME}}
sudo chmod 640 /etc/{{BINARY_NAME}}/env
```

### 数据库连接错误

1. **验证 PostgreSQL 正在运行:**
   ```bash
   sudo systemctl status postgresql
   ```

2. **检查连接字符串:**
   - Native 模式: 查看 `/etc/{{BINARY_NAME}}/env`
   - Docker 模式: 查看 `/etc/{{BINARY_NAME}}/docker/.env`

3. **确保数据库存在且用户有权限**

### 端口已被占用

修改配置文件中的端口:
```bash
# Native 模式 - 编辑 /etc/{{BINARY_NAME}}/env
OPS_SERVER__ADDR=0.0.0.0:3001

# Docker 模式 - 编辑 /etc/{{BINARY_NAME}}/docker/.env
# 然后修改 docker-compose.yml 中的端口映射
```

## 升级

### 使用更新脚本（推荐）

```bash
# 1. 解压新版本
tar -xzf {{BINARY_NAME}}-{{NEW_VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 2. 运行更新脚本（自动备份并更新）
sudo ./scripts/update.sh

# 3. 验证更新
./scripts/status.sh
```

### 手动升级

```bash
# 1. 备份当前版本
sudo ./scripts/backup.sh

# 2. 停止服务
sudo ./scripts/stop.sh

# 3. 替换二进制文件
sudo cp bin/{{BINARY_NAME}} /usr/local/bin/
sudo chmod +x /usr/local/bin/{{BINARY_NAME}}

# 4. 启动服务
sudo ./scripts/start.sh
```

## 卸载

### Docker 模式卸载

```bash
# 运行卸载脚本
sudo ./scripts/uninstall.sh

# 手动删除 Docker 卷（可选）
docker volume ls
docker volume rm <volume_name>
```

### Native 模式卸载

```bash
# 运行卸载脚本
sudo ./scripts/uninstall.sh

# 注意: 这将删除所有数据，包括:
# - 配置文件 /etc/{{BINARY_NAME}}
# - 数据目录 /var/lib/{{BINARY_NAME}}
# - 日志文件 /var/log/{{BINARY_NAME}}
# - 系统用户 {{BINARY_NAME}}
# - 数据库（需要手动删除）
```

### 手动删除数据库

```bash
sudo -u postgres psql
DROP DATABASE ops_service;
DROP USER ops_user;
\q
```

## 清洁安装

如果您需要完全重新开始:

```bash
# 清洁安装会删除所有数据并重新安装
sudo ./scripts/clean-install.sh

# 注意: 此操作不可逆，将永久删除所有数据！
```

## 默认账户

如果您在安装时选择了加载种子数据:

| 用户名 | 密码 | 角色 | 说明 |
|--------|------|------|------|
| admin | Admin123! | 管理员 | 完全访问权限 |
| demo | Demo123! | 操作员 | 受限访问权限 |

**⚠️ 重要: 首次登录后请立即修改默认密码！**

## 更多资源

- [Docker 部署指南](DOCKER_CN.md)
- [安全配置指南](SECURITY_CN.md)
- [故障排除指南](TROUBLESHOOTING_CN.md)
- [升级指南](UPGRADE_CN.md)

## 获取帮助

```bash
# 查看日志
sudo ./scripts/status.sh

# 检查系统资源
htop 或 top

# 查看完整日志
# Docker 模式
cd /etc/{{BINARY_NAME}}/docker && docker-compose logs

# Native 模式
sudo journalctl -u {{BINARY_NAME}} -f
```

## 下一步

- 查看日志确认服务正常运行
- 修改默认密码
- 配置反向代理（如 Nginx）
- 设置自动备份
- 配置监控和告警
- 查看安全最佳实践
