# {{BINARY_NAME}} 管理脚本说明

本目录包含用于管理 {{BINARY_NAME}} 服务的脚本。

## 📋 脚本分类

### 系统管理脚本

这些脚本用于管理 systemd 服务：

- **install.sh** - 安装 {{BINARY_NAME}} 为 systemd 服务
- **start.sh** - 启动服务 (增强版,包含数据库检测和配置验证)
- **stop.sh** - 停止服务
- **restart.sh** - 重启服务
- **status.sh** - 查看服务状态
- **uninstall.sh** - 卸载服务

### 运维脚本

- **check-db.sh** - 数据库检测与初始化 (新增)
- **backup.sh** - 备份数据库和配置文件
- **update.sh** - 更新到新版本

### Docker 相关脚本

Docker 相关脚本位于 `../docker/` 目录：

- **docker/build.sh** - 构建 Docker 镜像（多架构支持）
- **docker/docker-compose.yml** - Docker Compose 配置
- **docker/docker-compose.secure.yml** - 安全加固的 Docker Compose 配置

## 🚀 快速开始

### systemd 部署

```bash
# 安装为系统服务
sudo ./install.sh

# 检查数据库状态 (推荐)
sudo ./check-db.sh

# 启动服务 (增强版会自动检测配置和数据库)
sudo ./start.sh

# 查看状态
./status.sh
```

### Docker 部署

```bash
cd ../docker

# 构建镜像
./build.sh

# 使用 Docker Compose 启动
docker-compose up -d

# 或使用安全配置启动
docker-compose -f docker-compose.secure.yml up -d
```

## 🆕 新增功能

### 增强版 start.sh

启动脚本现已升级,包含以下特性:

#### 🔍 启动前检查:
1. ✅ 验证 systemd 服务文件
2. ✅ 验证配置文件存在性
3. ✅ **自动验证配置格式(检测环境变量命名)**
4. ✅ 提取并显示数据库连接信息
5. ✅ 检查 PostgreSQL 服务状态
6. ✅ **测试数据库连接**
7. ✅ 检查迁移文件
8. ✅ 自动重启已运行的服务
9. ✅ 重新加载 systemd 配置
10. ✅ 启动服务
11. ✅ 验证服务状态并显示详细日志

#### 特色功能:
- 🎨 **彩色输出**: 清晰的彩色日志便于快速定位问题
- 🔍 **配置格式验证**: 自动检测环境变量是否使用双下划线(__)格式
- 🔗 **数据库连接测试**: 启动前测试数据库是否可访问
- 📊 **详细状态显示**: 启动成功后显示完整的服务状态和访问信息
- ❌ **智能错误提示**: 失败时显示最近的日志和故障排除建议

### 数据库检测脚本 check-db.sh

独立的数据库检测工具,用于诊断和初始化数据库:

#### 🔧 功能:
1. ✅ 加载并解析配置文件
2. ✅ 检查 PostgreSQL 服务状态
3. ✅ **测试数据库服务器连接**
4. ✅ **自动创建不存在的数据库**
5. ✅ 测试目标数据库连接
6. ✅ 检查迁移状态
7. ✅ 列出迁移文件
8. ✅ 执行测试查询

#### 使用方法:
```bash
sudo ./check-db.sh
```

## ⚙️ 配置文件格式说明

### ⚠️ 重要:环境变量命名规则

配置文件必须使用**双下划线**(`__`)来分隔嵌套字段。

#### ✅ 正确格式:
```bash
# 数据库配置 (注意双下划线 __)
OPS_DATABASE__URL=postgresql://postgres:password@localhost:5432/ops_service
OPS_DATABASE__MAX_CONNECTIONS=10
OPS_DATABASE__MIN_CONNECTIONS=2

# 服务器配置
OPS_SERVER__ADDR=0.0.0.0:3000

# 安全配置
OPS_SECURITY__JWT_SECRET=your-secret-key-min-32-chars
OPS_SECURITY__RATE_LIMIT_RPS=100

# 日志配置
OPS_LOGGING__LEVEL=info
OPS_LOGGING__FORMAT=json
```

#### ❌ 错误格式:
```bash
# 错误: 使用单下划线 (无法被程序识别)
OPS_DATABASE_URL=postgresql://...
OPS_SERVER_ADDR=0.0.0.0:3000
OPS_SECURITY_JWT_SECRET=...
```

## 🐛 常见问题

### 问题 1: "missing configuration field 'database.url'"

**原因**: 配置文件使用了单下划线而不是双下划线

**解决方案**:
```bash
sudo nano /etc/ops-system/env
```

将所有单下划线改为双下划线:
- `OPS_DATABASE_URL` → `OPS_DATABASE__URL`
- `OPS_SERVER_ADDR` → `OPS_SERVER__ADDR`
- `OPS_SECURITY_JWT_SECRET` → `OPS_SECURITY__JWT_SECRET`

### 问题 2: 数据库连接失败

**使用检查脚本**:
```bash
sudo ./check-db.sh
```

该脚本会:
- 测试数据库连接
- 创建缺失的数据库
- 显示详细的错误信息

### 问题 3: 服务启动失败

**查看详细日志**:
```bash
# 查看服务状态
sudo systemctl status ops-system.service

# 查看最近的日志
sudo journalctl -u ops-system.service -n 100

# 实时跟踪日志
sudo journalctl -u ops-system.service -f
```

## 📝 典型工作流程

### 首次安装:

```bash
# 1. 安装服务
sudo ./install.sh

# 2. 编辑配置(如果需要)
sudo nano /etc/ops-system/env

# 3. 检查数据库(推荐)
sudo ./check-db.sh

# 4. 启动服务(增强版会自动检测)
sudo ./start.sh

# 5. 验证服务
curl http://localhost:3000/api/health
```

### 日常使用:

```bash
# 启动服务
sudo ./start.sh

# 或者使用 systemctl
sudo systemctl start ops-system.service

# 查看状态
sudo systemctl status ops-system.service

# 查看日志
sudo journalctl -u ops-system.service -f
```

### 更新配置后:

```bash
# 1. 编辑配置
sudo nano /etc/ops-system/env

# 2. 检查数据库(可选)
sudo ./check-db.sh

# 3. 重启服务
sudo systemctl restart ops-system.service

# 或使用启动脚本(会自动重启)
sudo ./start.sh
```

## 🔧 系统要求

### 必需:
- Linux 系统 (systemd)
- PostgreSQL 12+
- Root 权限

### 推荐:
- postgresql-client (用于数据库检测功能)

安装 PostgreSQL 客户端:
```bash
# Ubuntu/Debian
sudo apt install postgresql-client

# CentOS/RHEL
sudo yum install postgresql

# Arch Linux
sudo pacman -S postgresql
```

## 📖 详细文档

- [部署指南](../docs/DEPLOY.md) - 完整的部署说明
- [Docker 指南](../docker/README.md) - Docker 部署说明
- [安全检查清单](../docs/SECURITY.md) - 安全配置指南
- [故障排查](../docs/TROUBLESHOOTING.md) - 常见问题解决

## 🔧 脚本权限

所有脚本都应该有执行权限：

```bash
chmod +x *.sh
```

## 📝 注意事项

1. **systemd 脚本需要 root 权限**：install.sh, uninstall.sh, start.sh, check-db.sh 需要 sudo
2. **Docker 脚本需要 Docker 用户组**：确保当前用户在 docker 组中
3. **备份和更新脚本**：建议定期运行 backup.sh 备份数据
4. **配置文件格式**：务必使用双下划线(__)分隔嵌套配置字段

## 🆘 获取帮助

每个脚本都支持 `--help` 参数：

```bash
./install.sh --help
./backup.sh --help
```

---

版本: {{VERSION}}
平台: {{PLATFORM}}
