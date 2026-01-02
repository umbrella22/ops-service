# {{BINARY_NAME}} v{{VERSION}} - 文档导航

## 📚 文档目录

### 快速开始
- [中文快速部署指南](DEPLOY_CN.md) - 快速部署和配置指南
- [English Deployment Guide](DEPLOY.md) - Quick deployment and configuration

### Docker 部署
- [中文 Docker 部署指南](DOCKER_CN.md) - Docker 模式详细说明
- [English Docker Guide](DOCKER.md) - Docker mode detailed instructions

### 其他文档
- [安全配置指南](SECURITY.md) - Security configuration guide
- [故障排除指南](TROUBLESHOOTING.md) - Troubleshooting guide
- [升级指南](UPGRADE.md) - Version upgrade guide

## 🚀 快速开始

### 一键初始化（推荐）

```bash
# 解压
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-x86_64.tar.gz
cd linux-x86_64

# 运行一键初始化脚本
sudo ./init.sh
```

### 选择安装模式

{{BINARY_NAME}} 支持两种安装模式:

#### 1. Docker 模式（推荐）
- ✅ 环境隔离，易于管理
- ✅ 自动配置数据库
- ✅ 一键启动和停止
- 需要安装 Docker 和 Docker Compose

#### 2. Native 模式
- ✅ 资源占用少
- ✅ 可使用系统 PostgreSQL
- 需要手动配置数据库

## 📋 管理脚本

所有管理脚本都支持 Docker 和 Native 两种模式，会自动检测:

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

# 清洁安装（删除所有数据重新安装）
sudo ./scripts/clean-install.sh

# 卸载
sudo ./scripts/uninstall.sh
```

## 🔧 配置文件

### Docker 模式
- 配置文件: `/etc/{{BINARY_NAME}}/docker/.env`
- Compose 文件: `/etc/{{BINARY_NAME}}/docker/docker-compose.yml`

### Native 模式
- 配置文件: `/etc/{{BINARY_NAME}}/env`
- 服务文件: `/etc/systemd/system/{{BINARY_NAME}}.service`

## 📦 默认账户

如果安装时选择加载种子数据:

| 用户名 | 密码 | 角色 | 说明 |
|--------|------|------|------|
| admin | Admin123! | 管理员 | 完全访问权限 |
| demo | Demo123! | 操作员 | 受限访问权限 |

**⚠️ 重要: 首次登录后请立即修改默认密码！**

## 🎯 根据使用场景选择

### 我想快速测试和开发
→ 使用 **Docker 模式**，运行 `sudo ./init.sh` 并选择 Docker

### 我有现有的 PostgreSQL 服务器
→ 使用 **Native 模式**，运行 `sudo ./scripts/install.sh --native`

### 我需要生产环境部署
→ 使用 **Docker 模式**，参考 [Docker 部署指南](DOCKER_CN.md)

### 我系统资源有限
→ 使用 **Native 模式**，资源占用更少

## 🆘 获取帮助

### 查看日志
```bash
# Docker 模式
cd /etc/{{BINARY_NAME}}/docker && docker-compose logs -f

# Native 模式
sudo journalctl -u {{BINARY_NAME}} -f
```

### 检查状态
```bash
sudo ./scripts/status.sh
```

### 常见问题
- 服务无法启动? 查看 [故障排除指南](TROUBLESHOOTING.md)
- 数据库连接错误? 检查配置文件中的数据库 URL
- 端口被占用? 修改配置文件中的端口设置

## 📖 更多资源

- **部署指南**: [中文](DEPLOY_CN.md) | [English](DEPLOY.md)
- **Docker 指南**: [中文](DOCKER_CN.md) | [English](DOCKER.md)
- **安全配置**: [SECURITY.md](SECURITY.md)
- **故障排除**: [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- **升级指南**: [UPGRADE.md](UPGRADE.md)

## 🔗 相关链接

- 项目主页: [GitHub Repository](https://github.com/example)
- 问题反馈: [GitHub Issues](https://github.com/example/issues)
- 文档站点: [Documentation](https://docs.example.com)

---

**版本**: {{VERSION}}
**更新日期**: {{BUILD_DATE}}
