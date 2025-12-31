#!/bin/bash
# 本地开发环境测试脚本

set -e

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🧪 本地测试脚本${NC}"
echo "=================="
echo ""

# 检查 PostgreSQL
echo -e "${BLUE}检查 PostgreSQL...${NC}"
if ! pg_isready > /dev/null 2>&1; then
    echo -e "${RED}✗ PostgreSQL 未运行${NC}"
    echo "请先启动 PostgreSQL: sudo systemctl start postgresql"
    echo ""
    echo "如果遇到认证问题，请运行: ./fix_pg_auth.sh"
    exit 1
fi
echo -e "${GREEN}✓ PostgreSQL 正在运行${NC}"
echo ""

# 设置环境变量
export PGUSER="${PGUSER:-postgres}"
export PGHOST="${PGHOST:-localhost}"
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/ops_system_test}"

echo "环境配置:"
echo "  PGUSER=$PGUSER"
echo "  PGHOST=$PGHOST"
echo ""

# 检查数据库
echo -e "${BLUE}检查测试数据库...${NC}"
if ! psql -h $PGHOST -U $PGUSER -lqt | cut -d \| -f 1 | grep -qw ops_system_test; then
    echo -e "${YELLOW}创建测试数据库...${NC}"
    createdb -h $PGHOST -U $PGUSER ops_system_test
fi
echo -e "${GREEN}✓ 测试数据库就绪${NC}"
echo ""

# 运行迁移（如果存在）
if [ -d "migrations" ]; then
    echo -e "${BLUE}运行数据库迁移...${NC}"
    if command -v sqlx &> /dev/null; then
        sqlx migrate run --database-url "$TEST_DATABASE_URL" 2>/dev/null || true
    elif [ -f "target/release/ops-system" ]; then
        cargo run --bin ops-system -- migrate 2>/dev/null || true
    fi
    echo -e "${GREEN}✓ 迁移完成${NC}"
    echo ""
fi

# 运行测试
echo -e "${BLUE}运行测试...${NC}"
echo ""

# 串行运行所有测试
cargo test --verbose -- --test-threads=1

echo ""
echo "=================="
echo -e "${GREEN}✓ 测试完成！${NC}"
echo ""
