#!/bin/bash
# 测试运行脚本

set -e

echo "🧪 运维系统测试套件"
echo "===================="
echo ""

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 设置环境变量（需要在检查数据库之前设置）
export PGUSER="${PGUSER:-postgres}"
export PGHOST="${PGHOST:-localhost}"  # 强制使用 TCP 连接
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/ops_system_test}"

# 检查 PostgreSQL 是否运行
echo "📋 检查 PostgreSQL..."
if ! PGHOST=$PGHOST pg_isready -q; then
    echo -e "${RED}✗ PostgreSQL 未运行${NC}"
    echo "请先启动 PostgreSQL: sudo systemctl start postgresql"
    exit 1
fi
echo -e "${GREEN}✓ PostgreSQL 正在运行${NC}"
echo ""

# 检查测试数据库是否存在
echo "📋 检查测试数据库..."
if ! PGHOST=$PGHOST psql -U $PGUSER -lqt | cut -d \| -f 1 | grep -qw ops_system_test; then
    echo -e "${YELLOW}! 测试数据库不存在,正在创建...${NC}"
    PGHOST=$PGHOST createdb -U $PGUSER ops_system_test
    echo -e "${GREEN}✓ 测试数据库已创建${NC}"
else
    echo -e "${GREEN}✓ 测试数据库已存在${NC}"
fi
echo ""

# 显示数据库连接
echo "🔗 数据库连接: $TEST_DATABASE_URL"
echo "🔗 数据库用户: $PGUSER"
echo ""

# 运行迁移
echo "🔄 运行数据库迁移..."
cargo run --bin ops-system -- migrate 2>/dev/null || true
echo ""

# 运行测试
echo "🧪 运行测试..."
echo ""

# 运行单元测试
echo "▶️  运行单元测试..."
cargo test --lib --quiet
echo ""

# 运行集成测试
echo "▶️  运行集成测试..."
cargo test --test api_health_tests --quiet
cargo test --test api_auth_tests --quiet
cargo test --test service_tests --quiet
cargo test --test repository_tests --quiet
echo ""

echo "===================="
echo -e "${GREEN}✓ 所有测试通过!${NC}"
echo ""

# 显示测试统计
echo "📊 测试统计:"
cargo test --no-run --quiet 2>&1 | grep "Running" || echo "  (编译完成)"
echo ""

echo "💡 提示:"
echo "  - 运行单个测试: cargo test test_login_success"
echo "  - 显示测试输出: cargo test -- --nocapture"
echo "  - 运行特定测试文件: cargo test --test api_auth_tests"
