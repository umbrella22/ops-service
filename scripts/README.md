# 测试脚本说明

项目包含多个测试和构建脚本，适用于不同的场景。

## 📜 脚本列表

### 核心测试脚本

| 脚本 | 用途 | 使用场景 |
|-----|------|---------|
| `run_tests.sh` | 主测试脚本 | 标准测试流程 |
| `test_local.sh` | 本地快速测试 | 开发时快速验证 |
| `ci_test.sh` | CI 完整测试 | CI/CD 环境 |
| `test_docker.sh` | Docker 测试 | 容器化环境 |

### 工具脚本

| 脚本 | 用途 |
|-----|------|
| `fix_pg_auth.sh` | 修复 PostgreSQL 认证问题 |
| `setup_test_db.sh` | 交互式设置测试数据库 |

## 🚀 使用方式

### 标准测试（推荐）

```bash
# 使用脚本
./scripts/run_tests.sh

# 或使用 Make
make test-all
```

### 本地快速测试

```bash
./scripts/test_local.sh
```

### Docker 测试

```bash
# 使用脚本
./scripts/test_docker.sh

# 或使用 Make
make docker-test
```

### CI 测试

```bash
./scripts/ci_test.sh

# 或使用 Make
make ci
```

## 🔧 工具脚本

### fix_pg_auth.sh

**用途**: 修复 PostgreSQL 认证问题

**使用场景**:
- 遇到 "Peer authentication failed" 错误
- 需要为 postgres 用户设置密码
- 需要为当前用户创建 PostgreSQL 角色

**运行方式**:
```bash
./scripts/fix_pg_auth.sh
# 按提示选择认证方式
```

### setup_test_db.sh

**用途**: 交互式设置测试数据库环境

**运行方式**:
```bash
./scripts/setup_test_db.sh
# 按提示配置 PostgreSQL 用户和密码
```

## 📖 详细说明

### run_tests.sh

**功能**:
- ✅ 检查 PostgreSQL 状态
- ✅ 创建测试数据库（如不存在）
- ✅ 运行数据库迁移
- ✅ 运行单元测试
- ✅ 运行集成测试
- ✅ 显示测试统计

**输出**: 彩色，带进度提示

**环境变量**:
```bash
export PGUSER=postgres
export PGHOST=localhost
export TEST_DATABASE_URL="postgresql://postgres:postgres@localhost:5432/ops_system_test"
```

### test_local.sh

**功能**:
- ✅ 快速检查 PostgreSQL
- ✅ 创建测试数据库
- ✅ 运行迁移（如需要）
- ✅ 运行所有测试（串行）

**特点**: 简洁快速，适合本地开发

### ci_test.sh

**功能**:
- ✅ 完整的 CI 流程
- ✅ PostgreSQL 检查和设置
- ✅ 安装 sqlx-cli
- ✅ 创建数据库和迁移
- ✅ 代码格式检查 (`cargo fmt`)
- ✅ Clippy 代码检查
- ✅ 单元测试
- ✅ 集成测试
- ✅ Release 构建

**用途**: GitHub Actions 或本地模拟 CI

### test_docker.sh

**功能**:
- ✅ 启动 PostgreSQL Docker 容器
- ✅ 等待容器就绪
- ✅ 运行测试
- ✅ 自动清理容器

**依赖**: `docker-compose.test.yml`

## 🔄 Make 命令对应关系

```bash
make test-all    # → ./scripts/run_tests.sh
make test-unit   # → cargo test --lib
make ci          # → ./scripts/ci_test.sh
make docker-test # → ./scripts/test_docker.sh
make setup-env   # → ./scripts/setup_test_db.sh
```

## 🛠️ 脚本开发

### 添加新脚本

1. 创建脚本文件: `scripts/new_script.sh`
2. 添加执行权限: `chmod +x scripts/new_script.sh`
3. 更新 Makefile 添加对应的 make 目标
4. 更新本文档

### 脚本规范

- 使用 `set -e` 遇错立即退出
- 添加彩色输出（GREEN/RED/YELLOW）
- 提供清晰的进度提示
- 包含使用说明

### 脚本模板

```bash
#!/bin/bash
# 脚本说明

set -e

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}✓ 操作成功${NC}"
echo -e "${RED}✗ 操作失败${NC}"
```

## 📝 相关文档

- [../USER_GUIDE.md](../USER_GUIDE.md) - 完整使用指南
- [../README.md](../README.md) - 项目说明
- [../Makefile](../Makefile) - Make 命令

## 🆘 常见问题

### 脚本权限问题

```bash
# 添加执行权限
chmod +x scripts/*.sh
```

### PostgreSQL 连接问题

```bash
# 运行修复脚本
./scripts/fix_pg_auth.sh
```

### Docker 测试失败

```bash
# 检查 Docker 是否运行
docker ps

# 重新启动 Docker 环境
make docker-down
make docker-up
make docker-test
```

## ⚡ 快速参考

```bash
# 标准测试
./scripts/run_tests.sh

# 快速测试
./scripts/test_local.sh

# CI 测试
./scripts/ci_test.sh

# Docker 测试
./scripts/test_docker.sh

# 修复认证
./scripts/fix_pg_auth.sh

# 设置环境
./scripts/setup_test_db.sh
```
