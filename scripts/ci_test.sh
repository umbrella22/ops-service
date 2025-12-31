#!/bin/bash
# CI 测试脚本 - 用于 GitHub Actions 或本地 CI

set -e

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🚀 CI 测试流程${NC}"
echo "=================="
echo ""

# 导出环境变量
export PGUSER="${PGUSER:-postgres}"
export PGHOST="${PGHOST:-localhost}"
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/ops_system_test}"

echo -e "${BLUE}环境变量:${NC}"
echo "  PGUSER=$PGUSER"
echo "  PGHOST=$PGHOST"
echo "  TEST_DATABASE_URL=$TEST_DATABASE_URL"
echo ""

# 1. 检查 PostgreSQL
echo -e "${BLUE}[1/6]${NC} 检查 PostgreSQL..."
if ! pg_isready -h $PGHOST -U $PGUSER > /dev/null 2>&1; then
    echo -e "${RED}✗ PostgreSQL 不可用${NC}"
    exit 1
fi
echo -e "${GREEN}✓ PostgreSQL 就绪${NC}"
echo ""

# 2. 安装 sqlx-cli
echo -e "${BLUE}[2/6]${NC} 安装 sqlx-cli..."
if ! command -v sqlx &> /dev/null; then
    cargo install sqlx-cli --no-default-features --features rustls,postgres
fi
echo -e "${GREEN}✓ sqlx-cli 已安装${NC}"
echo ""

# 3. 创建测试数据库
echo -e "${BLUE}[3/6]${NC} 设置测试数据库..."
sqlx database create --database-url "$TEST_DATABASE_URL" 2>/dev/null || echo "  数据库已存在"
echo -e "${GREEN}✓ 测试数据库就绪${NC}"
echo ""

# 4. 运行迁移
echo -e "${BLUE}[4/6]${NC} 运行数据库迁移..."
sqlx migrate run --database-url "$TEST_DATABASE_URL"
echo -e "${GREEN}✓ 迁移完成${NC}"
echo ""

# 5. 代码检查
echo -e "${BLUE}[5/6]${NC} 代码质量检查..."
echo "  运行 fmt 检查..."
cargo fmt -- --check
echo -e "${GREEN}  ✓ 格式检查通过${NC}"

echo "  运行 clippy..."
cargo clippy -- -D warnings
echo -e "${GREEN}  ✓ Clippy 检查通过${NC}"
echo ""

# 6. 运行测试
echo -e "${BLUE}[6/6]${NC} 运行测试套件..."
echo ""

# 单元测试
echo -e "${YELLOW}  运行单元测试...${NC}"
cargo test --lib -- --test-threads=1
echo -e "${GREEN}  ✓ 单元测试通过${NC}"
echo ""

# 集成测试
echo -e "${YELLOW}  运行集成测试...${NC}"
cargo test --test api_health_tests -- --test-threads=1
cargo test --test api_auth_tests -- --test-threads=1
cargo test --test service_tests -- --test-threads=1
cargo test --test repository_tests -- --test-threads=1
echo -e "${GREEN}  ✓ 集成测试通过${NC}"
echo ""

# 7. 构建 release
echo -e "${BLUE}[7/7]${NC} 构建 release 版本..."
cargo build --release
echo -e "${GREEN}✓ 构建成功${NC}"
echo ""

# 总结
echo "=================="
echo -e "${GREEN}✅ 所有检查通过！${NC}"
echo ""
echo "测试结果:"
echo "  ✓ PostgreSQL 连接"
echo "  ✓ 数据库迁移"
echo "  ✓ 代码格式"
echo "  ✓ Clippy 检查"
echo "  ✓ 单元测试"
echo "  ✓ 集成测试"
echo "  ✓ Release 构建"
echo ""
