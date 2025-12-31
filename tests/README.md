# 测试套件

这是一套完整的自动化测试套件,用于验证运维系统的功能正确性。

## 测试结构

```
tests/
├── common/                   # 测试公共模块
│   └── mod.rs               # 测试工具函数
├── api_health_tests.rs      # 健康检查 API 测试
├── api_auth_tests.rs        # 认证 API 测试
├── service_tests.rs         # 服务层测试
└── repository_tests.rs      # 仓库层测试
```

## 测试覆盖范围

### 1. 集成测试 (API层)

#### `api_health_tests.rs` - 健康检查测试
- ✅ 健康检查端点 `/health`
- ✅ 就绪检查端点 `/ready`
- ✅ 指标端点 `/metrics`
- ✅ 404 错误处理

#### `api_auth_tests.rs` - 认证 API 测试
- ✅ 用户登录成功
- ✅ 密码错误登录失败
- ✅ 用户不存在登录失败
- ✅ 获取当前用户信息
- ✅ 无 token 访问受保护端点
- ✅ 用户登出

### 2. 单元测试 (服务层)

#### `service_tests.rs` - 服务层测试
- ✅ AuthService 登录成功
- ✅ AuthService 密码错误
- ✅ AuthService 登录速率限制
- ✅ AuthService 刷新令牌
- ✅ PermissionService 权限检查
- ✅ AuditService 审计日志记录
- ✅ JwtService 令牌生成和验证
- ✅ JwtService 无效令牌验证

### 3. 单元测试 (仓库层)

#### `repository_tests.rs` - 仓库层测试
- ✅ UserRepository 创建和查找用户
- ✅ UserRepository 按 ID 查找
- ✅ UserRepository 更新失败登录次数
- ✅ RoleRepository 创建和查找角色
- ✅ AssetRepository 创建资产组
- ✅ AssetRepository 创建主机
- ✅ AssetRepository 列出主机
- ✅ AuditRepository 日志记录和检索
- ✅ AuditRepository 日志计数

## 运行测试

### 前置条件

1. 确保已安装 PostgreSQL
2. 创建测试数据库:
   ```bash
   createdb ops_system_test
   ```

### 设置环境变量

```bash
export TEST_DATABASE_URL="postgresql://postgres:postgres@localhost:5432/ops_system_test"
```

或者直接使用默认的测试数据库连接字符串。

### 运行所有测试

```bash
# 运行所有测试
cargo test

# 运行测试并显示输出
cargo test -- --nocapture

# 运行测试并显示详细信息
cargo test -- --show-output
```

### 运行特定测试

```bash
# 运行 API 测试
cargo test --test api_health_tests
cargo test --test api_auth_tests

# 运行服务层测试
cargo test --test service_tests

# 运行仓库层测试
cargo test --test repository_tests

# 运行特定测试函数
cargo test test_login_success
```

### 忽略需要数据库的测试

某些测试需要数据库连接,可以临时跳过:

```bash
# 跳过需要数据库的测试
cargo test --ignore
```

## 测试数据库管理

### 清理测试数据

测试运行后会自动清理数据。如果需要手动清理:

```bash
psql -U postgres -d ops_system_test -c "TRUNCATE TABLE audit_logs, refresh_tokens, assets_hosts, asset_groups, users, roles CASCADE;"
```

### 重新创建测试数据库

```bash
dropdb ops_system_test
createdb ops_system_test
```

## 持续集成

测试配置在项目根目录的 `.github/workflows/test.yml` (如果存在)。

CI 流程会:
1. 设置 Rust 环境
2. 启动 PostgreSQL 容器
3. 运行数据库迁移
4. 执行所有测试
5. 生成代码覆盖率报告

## 代码覆盖率

安装 tarpaulin 来生成覆盖率报告:

```bash
cargo install cargo-tarpaulin

# 生成覆盖率报告
cargo tarpaulin --out Html --output-dir coverage

# 查看报告
open coverage/index.html
```

## 最佳实践

1. **隔离性**: 每个测试独立运行,不依赖其他测试的状态
2. **清理**: 测试完成后清理数据库,避免影响其他测试
3. **幂等性**: 多次运行同一测试应该得到相同结果
4. **快速**: 单元测试应该快速执行
5. **明确**: 测试名称应该清楚描述测试内容

## 添加新测试

1. 在相应的测试文件中添加新的 `#[tokio::test]` 函数
2. 使用 `common` 模块中的工具函数
3. 遵循命名约定: `test_<功能>_<场景>_<期望结果>`
4. 确保测试后清理数据

示例:

```rust
#[tokio::test]
async fn test_user_delete_success() {
    let config = common::create_test_config();
    let pool = common::setup_test_db(&config).await;

    // 准备测试数据
    let user_id = create_test_user(&pool, "testuser", "TestPass123", "test@example.com")
        .await
        .expect("Failed to create test user");

    // 执行测试操作
    // ...

    // 验证结果
    assert_eq!(result, expected);

    // 清理会自动进行
}
```

## 故障排查

### 测试失败: "Connection refused"

确保 PostgreSQL 正在运行:
```bash
# 检查 PostgreSQL 状态
sudo systemctl status postgresql

# 启动 PostgreSQL
sudo systemctl start postgresql
```

### 测试失败: "Database does not exist"

创建测试数据库:
```bash
createdb ops_system_test
```

### 测试超时

增加测试超时时间:
```bash
cargo test -- --test-threads=1
```

## 贡献指南

提交代码前请确保:
1. 所有测试通过
2. 新功能包含对应的测试
3. 代码覆盖率没有降低
4. 遵循项目的编码规范
