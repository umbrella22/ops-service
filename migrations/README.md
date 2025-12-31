# 数据库迁移指南

## 📋 目录

- [概述](#概述)
- [迁移文件说明](#迁移文件说明)
- [使用方法](#使用方法)
- [数据库结构](#数据库结构)
- [初始数据](#初始数据)
- [最佳实践](#最佳实践)

## 概述

本项目使用 PostgreSQL 数据库，采用版本化的迁移脚本管理系统。每个迁移脚本都有唯一的版本号，确保数据库架构的可追溯性和可重复性。

### 迁移文件命名规则

```
<VVERSION>_<DESCRIPTION>.sql
```

例如：
- `000001_init_baseline.sql` - 初始化基线表
- `000002_p1_identity_and_audit.sql` - 身份认证和审计表
- `000003_seed_data.sql` - 初始数据

## 迁移文件说明

### 1. `000001_init_baseline.sql` - 基线表

**用途**: 创建基础健康检查表

**内容**:
```sql
-- 健康检查表（用于数据库连接测试）
CREATE TABLE health_check (
    id SERIAL PRIMARY KEY,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### 2. `000002_p1_identity_and_audit.sql` - 核心业务表

**用途**: 创建所有核心业务表

**包含的表**:

#### 身份认证 (Identity)
- `users` - 用户表（账户状态、安全策略）
- `roles` - 角色表
- `permissions` - 权限表（资源+操作）
- `role_permissions` - 角色-权限关联表
- `role_bindings` - 用户角色绑定（支持范围限制）
- `api_keys` - API密钥（服务账户）
- `refresh_tokens` - 刷新令牌（令牌轮换）

#### 资产管理 (Assets)
- `assets_groups` - 资产组（层级结构、环境感知）
- `assets_hosts` - 主机资产

#### 审计日志 (Audit)
- `audit_logs` - 操作审计日志
- `login_events` - 登录事件（安全监控）

**特性**:
- ✅ 自动时间戳更新 (`updated_at`)
- ✅ 乐观锁版本控制 (`version`)
- ✅ 自动审计触发器
- ✅ 完整索引优化
- ✅ 外键约束和级联规则

## 使用方法

### 🐳 方式一：Docker 部署（推荐）

**适合**：快速部署、生产环境

```bash
# 启动服务（自动运行所有迁移）
docker-compose up -d

# 查看迁移日志
docker-compose logs api | grep migration

# 查看数据库
docker-compose exec postgres psql -U ops_user -d ops_system
```

**优势**：
- ✅ 零配置，自动完成所有迁移
- ✅ 环境隔离，不影响主机
- ✅ 自动加载种子数据

详细说明：[QUICKSTART.md](QUICKSTART.md#方式一docker-部署推荐)

---

### 🔧 方式二：Native 部署

**适合**：开发环境、已有 PostgreSQL、需要定制化

#### 方法 A：使用迁移管理脚本（推荐）

```bash
# 查看迁移状态
./scripts/migrate.sh status

# 运行所有迁移
./scripts/migrate.sh migrate

# 加载种子数据（可选）
./scripts/migrate.sh seed

# 进入数据库
./scripts/migrate.sh shell
```

#### 方法 B：使用 sqlx-cli

```bash
# 安装 sqlx-cli
cargo install sqlx-cli --no-default-features --features rustls,postgres

# 设置数据库 URL
export DATABASE_URL="postgresql://postgres:password@localhost:5432/ops_system"

# 运行迁移
sqlx migrate run --source migrations

# 查看状态
sqlx migrate info --database-url $DATABASE_URL
```

#### 方法 C：使用 psql 手动执行

```bash
# 连接到数据库
psql -U postgres -d ops_system

# 按顺序执行迁移文件
\i migrations/000001_init_baseline.sql
\i migrations/000002_p1_identity_and_audit.sql
\i migrations/000003_seed_data.sql  -- 可选
```

#### 方法 D：应用自动迁移

```bash
# 设置环境变量
export OPS_DATABASE__URL="postgresql://user:pass@localhost:5432/ops_system"

# 启动应用（自动运行未执行的迁移）
./ops-system
```

详细说明：[QUICKSTART.md](QUICKSTART.md#方式二native-部署)

## 数据库结构

### 表关系图

```
users (用户)
  ├── role_bindings (角色绑定) ←→ roles (角色)
  │                              └── role_permissions (权限) ←→ permissions
  ├── api_keys (API密钥)
  ├── refresh_tokens (刷新令牌)
  └── created_by ──────┐
                       │
assets_groups (资产组)  │
  ├── parent_id (自引用)│
  └── assets_hosts (主机资产)
                          │
login_events ─────────────┘
audit_logs
```

### 主要字段说明

#### users 表
- `status`: 账户状态 (enabled/disabled/locked)
- `failed_login_attempts`: 失败登录次数
- `must_change_password`: 强制修改密码标志
- `version`: 乐观锁版本号

#### assets_hosts 表
- `identifier`: 唯一标识符
- `group_id`: 所属资产组
- `environment`: 环境 (dev/stage/prod)
- `tags`: JSONB 数组，支持标签搜索
- `status`: 主机状态

#### audit_logs 表
- `subject_id`: 操作者 ID
- `action`: 操作类型 (create/update/delete/execute)
- `changes`: JSONB 格式的变更详情
- `result`: 操作结果 (success/failure/partial)

## 初始数据

### 默认权限

系统预置以下权限：

| 资源 | 操作 | 说明 |
|------|------|------|
| asset | read | 查看资产和组 |
| asset | write | 创建、更新、删除资产 |
| job | read | 查看任务和作业 |
| job | execute | 执行任务 |
| job | approve | 批准生产环境任务 |
| audit | read | 查看审计日志 |
| audit | admin | 系统级审计访问 |
| user | read | 查看用户信息 |
| user | write | 管理用户和角色 |
| system | admin | 系统管理 |

### 默认角色

| 角色名 | 说明 | 权限 |
|--------|------|------|
| admin | 系统管理员 | 全部权限 |
| operator | 操作员 | 读取+执行权限 |
| viewer | 查看者 | 仅读取权限 |
| auditor | 审计员 | 审计日志读取 |

### 默认管理员账户

```
用户名: admin
邮箱: admin@ops-system.local
密码: Admin123!
状态: 启用，首次登录需修改密码
```

**安全提示**: 生产环境请立即修改默认密码！

## 最佳实践

### 1. 迁移脚本编写规则

- ✅ 使用 `IF NOT EXISTS` 确保幂等性
- ✅ 每个迁移文件只做一件事
- ✅ 添加详细的注释说明
- ✅ 使用事务确保原子性
- ❌ 避免修改已存在的迁移文件

### 2. 创建新迁移

```bash
# 使用 sqlx-cli 创建新迁移
sqlx migrate add add_user_preferences_table
```

这会创建两个文件：
- `migrations/XXXXXX_add_user_preferences_table.up.sql`
- `migrations/XXXXXX_add_user_preferences_table.down.sql`

### 3. 索引优化

- 为常查询字段创建索引
- JSONB 字段使用 GIN 索引
- 复合索引注意字段顺序

```sql
-- 示例：为标签字段创建 GIN 索引
CREATE INDEX idx_assets_hosts_tags ON assets_hosts USING GIN(tags);

-- 示例：复合索引
CREATE INDEX idx_audit_logs_subject_time
ON audit_logs(subject_id, occurred_at DESC);
```

### 4. 审计触发器

系统为关键表配置了自动审计触发器：

```sql
-- assets_hosts 表的审计会在 INSERT/UPDATE/DELETE 时自动记录
-- 审计记录包括：操作者、操作类型、变更内容、时间戳
```

### 5. 数据库备份

```bash
# 备份数据库
pg_dump -U postgres -d ops_system -F c -f backup_$(date +%Y%m%d).dump

# 恢复数据库
pg_restore -U postgres -d ops_system backup.dump
```

### 6. 性能监控

```sql
-- 查看慢查询
SELECT query, mean_exec_time, calls
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 10;

-- 查看表大小
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) AS size
FROM pg_tables
WHERE schemaname = 'public'
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;

-- 查看索引使用情况
SELECT
    schemaname,
    tablename,
    indexname,
    idx_scan as index_scans
FROM pg_stat_user_indexes
ORDER BY idx_scan ASC;
```

## 常见问题

### Q: 如何重置数据库？

```bash
# 删除所有表
psql -U postgres -d ops_system -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"

# 重新运行迁移
sqlx migrate run --database-url $DATABASE_URL
```

### Q: 如何查看已执行的迁移？

```bash
sqlx migrate info --database-url $DATABASE_URL
```

或在数据库中查询：

```sql
SELECT * FROM _sqlx_migrations ORDER BY version;
```

### Q: 迁移失败怎么办？

1. 查看错误信息确定失败原因
2. 修复问题后，手动回滚：
   ```bash
   sqlx migrate revert --database-url $DATABASE_URL
   ```
3. 重新运行迁移

### Q: 如何在生产环境安全执行迁移？

1. **先在测试环境验证**
2. **备份数据库**
3. **使用事务确保可回滚**
4. **分阶段执行（先只读迁移，再写入迁移）**
5. **监控应用性能**

## 参考资源

- [PostgreSQL 文档](https://www.postgresql.org/docs/)
- [SQLx 迁移文档](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli)
- [数据库设计最佳实践](https://www.postgresql.org/docs/current/ddl-constraints.html)
