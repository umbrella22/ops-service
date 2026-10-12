# SQL 速查表

## 📋 目录

- [用户管理](#用户管理)
- [权限和角色](#权限和角色)
- [资产管理](#资产管理)
- [审计日志](#审计日志)
- [统计查询](#统计查询)
- [维护操作](#维护操作)

## 用户管理

### 创建用户

```sql
-- 创建新用户
INSERT INTO users (username, email, password_hash, full_name, department, status)
VALUES (
    'jane.doe',
    'jane.doe@example.com',
    '$argon2id$v=19$m=65536,t=3,p=2$...', -- 使用 Argon2 哈希
    'Jane Doe',
    'Engineering',
    'enabled'
);

-- 创建用户并分配角色
DO $$
DECLARE
    new_user_id UUID;
    operator_role_id UUID;
BEGIN
    -- 创建用户
    INSERT INTO users (username, email, password_hash, full_name)
    VALUES ('bob', 'bob@example.com', '...')
    RETURNING id INTO new_user_id;

    -- 获取角色 ID
    SELECT id INTO operator_role_id FROM roles WHERE name = 'operator';

    -- 分配角色
    INSERT INTO role_bindings (user_id, role_id, scope_type)
    VALUES (new_user_id, operator_role_id, 'global');
END $$;
```

### 查询用户

```sql
-- 查看所有用户
SELECT id, username, email, full_name, status, created_at
FROM users
ORDER BY created_at DESC;

-- 查看特定用户
SELECT * FROM users WHERE username = 'admin';

-- 查看用户及其角色
SELECT
    u.username,
    u.full_name,
    u.status,
    r.name as role_name,
    rb.scope_type
FROM users u
LEFT JOIN role_bindings rb ON u.id = rb.user_id
LEFT JOIN roles r ON rb.role_id = r.id
ORDER BY u.username;
```

### 更新用户

```sql
-- 启用/禁用用户
UPDATE users SET status = 'disabled' WHERE username = 'jane.doe';

-- 强制修改密码
UPDATE users SET
    must_change_password = TRUE
WHERE username = 'jane.doe';

-- 重置失败登录次数
UPDATE users SET
    failed_login_attempts = 0,
    locked_until = NULL
WHERE username = 'jane.doe';
```

### 删除用户

```sql
-- 删除用户（会自动删除相关的角色绑定）
DELETE FROM users WHERE username = 'jane.doe';
```

## 权限和角色

### 查看权限

```sql
-- 查看所有权限
SELECT resource, action, description
FROM permissions
ORDER BY resource, action;

-- 查看角色的权限
SELECT
    r.name as role,
    p.resource,
    p.action,
    p.description
FROM roles r
JOIN role_permissions rp ON r.id = rp.role_id
JOIN permissions p ON rp.permission_id = p.id
WHERE r.name = 'admin'
ORDER BY p.resource, p.action;

-- 查看用户的所有权限
SELECT DISTINCT
    u.username,
    p.resource,
    p.action
FROM users u
JOIN role_bindings rb ON u.id = rb.user_id
JOIN role_permissions rp ON rb.role_id = rp.role_id
JOIN permissions p ON rp.permission_id = p.id
WHERE u.username = 'admin'
ORDER BY p.resource, p.action;
```

### 创建角色和权限

```sql
-- 创建新角色
INSERT INTO roles (name, description, is_system)
VALUES ('deployer', 'Can deploy applications', FALSE);

-- 创建新权限
INSERT INTO permissions (resource, action, description)
VALUES ('deployment', 'execute', 'Execute deployment jobs');

-- 分配权限给角色
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.name = 'deployer' AND p.resource = 'deployment' AND p.action = 'execute';
```

### 分配角色

```sql
-- 分配全局角色
INSERT INTO role_bindings (user_id, role_id, scope_type)
SELECT u.id, r.id, 'global'
FROM users u, roles r
WHERE u.username = 'jane.doe' AND r.name = 'operator';

-- 分配环境范围的角色
INSERT INTO role_bindings (user_id, role_id, scope_type, scope_value)
SELECT u.id, r.id, 'environment', 'prod'
FROM users u, roles r
WHERE u.username = 'jane.doe' AND r.name = 'viewer';

-- 分配资产组范围的角色
INSERT INTO role_bindings (user_id, role_id, scope_type, scope_value)
SELECT u.id, r.id, 'group', '<group-id>'
FROM users u, roles r
WHERE u.username = 'jane.doe' AND r.name = 'operator';
```

## 资产管理

### 创建资产组

```sql
-- 创建资产组
INSERT INTO assets_groups (name, description, environment)
VALUES ('prod-servers', 'Production Servers', 'prod');

-- 创建子资产组
-- （SELECT 列表必须是列表达式，不能是行构造器）
INSERT INTO assets_groups (name, description, environment, parent_id)
SELECT 'web-servers', 'Web Servers', 'prod', id
FROM assets_groups WHERE name = 'prod-servers' AND environment = 'prod';
```

### 创建主机

```sql
-- 创建主机
INSERT INTO assets_hosts (
    identifier, display_name, address, port,
    group_id, environment, tags,
    owner_id, status, os_type, os_version, notes
)
VALUES (
    'web-01',
    'Web Server 01',
    '192.168.1.10',
    22,
    (SELECT id FROM assets_groups WHERE name = 'prod-servers' AND environment = 'prod'),
    'prod',
    '["web", "nginx", "production"]'::JSONB,
    (SELECT id FROM users WHERE username = 'admin'),
    'active',
    'Ubuntu',
    '22.04 LTS',
    'Main web server'
);
```

### 查询主机

```sql
-- 查看所有主机
SELECT
    identifier,
    display_name,
    address,
    environment,
    status,
    os_type,
    created_at
FROM assets_hosts
ORDER BY environment, identifier;

-- 按环境查看主机
SELECT
    environment,
    status,
    COUNT(*) as count
FROM assets_hosts
GROUP BY environment, status
ORDER BY environment, status;

-- 搜索带特定标签的主机
SELECT identifier, display_name, tags
FROM assets_hosts
WHERE tags @> '["web"]'::JSONB;

-- 查看特定资产组的主机
SELECT
    h.identifier,
    h.display_name,
    h.address,
    h.status
FROM assets_hosts h
JOIN assets_groups g ON h.group_id = g.id
WHERE g.name = 'prod-servers' AND g.environment = 'prod';

-- 查看主机及其所属组
SELECT
    h.identifier,
    h.display_name,
    g.name as group_name,
    h.environment,
    h.status
FROM assets_hosts h
JOIN assets_groups g ON h.group_id = g.id
ORDER BY h.environment, h.identifier;
```

### 更新主机

```sql
-- 更新主机状态
UPDATE assets_hosts
SET status = 'maintenance'
WHERE identifier = 'web-01';

-- 添加标签
UPDATE assets_hosts
SET tags = tags || '["loadbalancer"]'::JSONB
WHERE identifier = 'web-01';

-- 移除指定标签（集合返回函数不能直接用于 UPDATE SET，
-- 需先过滤再重新聚合）
UPDATE assets_hosts
SET tags = (
    SELECT COALESCE(jsonb_agg(elem), '[]'::JSONB)
    FROM JSONB_ARRAY_ELEMENTS_TEXT(tags) AS elem
    WHERE elem <> 'loadbalancer'
)
WHERE identifier = 'web-01';
```

### 删除主机

```sql
-- 删除单个主机
DELETE FROM assets_hosts WHERE identifier = 'web-01';

-- 批量删除
DELETE FROM assets_hosts WHERE status = 'decommissioned';
```

## 审计日志

### 查询审计日志

```sql
-- 查看最近的活动
SELECT
    occurred_at,
    subject_name,
    action,
    resource_name,
    changes_summary
FROM audit_logs
ORDER BY occurred_at DESC
LIMIT 50;

-- 查看特定用户的操作
SELECT
    occurred_at,
    action,
    resource_name,
    result,
    changes_summary
FROM audit_logs
WHERE subject_id = '<user-id>'
ORDER BY occurred_at DESC;

-- 查看失败的操作
SELECT
    occurred_at,
    subject_name,
    action,
    resource_name,
    error_message
FROM audit_logs
WHERE result = 'failure'
ORDER BY occurred_at DESC;

-- 查看特定资源的操作历史
SELECT
    occurred_at,
    subject_name,
    action,
    result,
    changes_summary
FROM audit_logs
WHERE resource_type = 'asset_host'
  AND resource_id = '<host-id>'
ORDER BY occurred_at DESC;

-- 按时间范围查询
SELECT * FROM audit_logs
WHERE occurred_at BETWEEN NOW() - INTERVAL '7 days' AND NOW()
ORDER BY occurred_at DESC;
```

### 查询登录事件

```sql
-- 查看最近的登录
SELECT
    occurred_at,
    username,
    event_type,
    source_ip,
    user_agent
FROM login_events
ORDER BY occurred_at DESC
LIMIT 20;

-- 查看失败的登录
SELECT
    occurred_at,
    username,
    failure_reason,
    source_ip
FROM login_events
WHERE event_type = 'login_failure'
ORDER BY occurred_at DESC;

-- 查看可疑活动
SELECT
    occurred_at,
    username,
    event_type,
    source_ip,
    risk_tag
FROM login_events
WHERE risk_tag IS NOT NULL
ORDER BY occurred_at DESC;
```

## 统计查询

### 用户统计

```sql
-- 按状态统计用户
SELECT status, COUNT(*) as count
FROM users
GROUP BY status;

-- 按部门统计用户
SELECT department, COUNT(*) as count
FROM users
GROUP BY department
ORDER BY count DESC;

-- 需要修改密码的用户
SELECT username, email, created_at
FROM users
WHERE must_change_password = TRUE;
```

### 主机统计

```sql
-- 使用统计视图
SELECT * FROM v_host_stats;

-- 按环境统计主机
SELECT environment, status, COUNT(*)
FROM assets_hosts
GROUP BY environment, status
ORDER BY environment, status;

-- 按标签统计
SELECT
    jsonb_array_elements_text(tags) as tag,
    COUNT(*) as count
FROM assets_hosts
GROUP BY tag
ORDER BY count DESC;
```

### 审计统计

```sql
-- 按操作类型统计
SELECT action, COUNT(*) as count
FROM audit_logs
GROUP BY action
ORDER BY count DESC;

-- 按结果统计
SELECT result, COUNT(*) as count
FROM audit_logs
GROUP BY result;

-- 最活跃的用户
SELECT subject_name, COUNT(*) as action_count
FROM audit_logs
GROUP BY subject_name
ORDER BY action_count DESC
LIMIT 10;
```

## 维护操作

### 数据库备份

```bash
# 使用迁移脚本
./scripts/migrate.sh backup

# 或手动备份
pg_dump -U postgres -d ops_service -F c -f backup.dump
```

### 数据库恢复

```bash
# 使用迁移脚本
./scripts/migrate.sh restore backup.dump

# 或手动恢复
pg_restore -U postgres -d ops_service backup.dump
```

### 清理旧数据

```sql
-- 删除旧的审计日志（保留最近 90 天）
DELETE FROM audit_logs
WHERE occurred_at < NOW() - INTERVAL '90 days';

-- 删除旧的登录事件（保留最近 30 天）
DELETE FROM login_events
WHERE occurred_at < NOW() - INTERVAL '30 days';

-- 清理已过期的刷新令牌
DELETE FROM refresh_tokens
WHERE expires_at < NOW() OR revoked_at IS NOT NULL;
```

### 性能优化

```sql
-- 分析表以更新统计信息
ANALYZE users;
ANALYZE assets_hosts;
ANALYZE audit_logs;

-- 重建索引
REINDEX TABLE audit_logs;

-- 清理死元组
VACUUM ANALYZE audit_logs;
```

### 监控查询

```sql
-- 查看表大小
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
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

-- 查看慢查询（需要 pg_stat_statements 扩展）
SELECT
    query,
    mean_exec_time,
    calls
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 10;
```

## 有用函数

### 检查用户权限

```sql
-- 获取用户的所有权限
SELECT * FROM get_user_permissions('<user-id>');

-- 检查用户是否有特定权限
SELECT check_permission('<user-id>', 'asset', 'read');
```

### 数据完整性

```sql
-- 检查孤儿主机（所有者不存在）
SELECT * FROM check_orphan_hosts();

-- 检查孤立的角色绑定
SELECT rb.id, u.username
FROM role_bindings rb
LEFT JOIN users u ON rb.user_id = u.id
WHERE u.id IS NULL;
```

## 常用模式

### 事务处理

```sql
BEGIN;

-- 创建用户
INSERT INTO users (username, email, password_hash) ...;

-- 分配角色
INSERT INTO role_bindings (user_id, role_id, scope_type) ...;

-- 创建主机
INSERT INTO assets_hosts (...) ...;

COMMIT;
-- 或出错时 ROLLBACK;
```

### 批量操作

```sql
-- 批量插入
INSERT INTO users (username, email, password_hash) VALUES
    ('user1', 'user1@example.com', '...'),
    ('user2', 'user2@example.com', '...'),
    ('user3', 'user3@example.com', '...');

-- 批量更新
UPDATE assets_hosts
SET status = 'active'
WHERE identifier IN ('host1', 'host2', 'host3');
```

### 条件操作

```sql
-- 仅当用户不存在时创建
INSERT INTO users (username, email, password_hash)
VALUES ('newuser', 'new@example.com', '...')
ON CONFLICT (username) DO NOTHING;

-- 更新或插入
INSERT INTO assets_hosts (identifier, display_name, address)
VALUES ('host1', 'Host 1', '192.168.1.1')
ON CONFLICT (identifier) DO UPDATE
SET display_name = EXCLUDED.display_name,
    address = EXCLUDED.address;
```
