#!/bin/bash
# Docker Compose 测试环境
# 用于在 Docker 容器中运行测试

set -e

GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🐳 Docker 测试环境${NC}"
echo "=================="
echo ""

# 检查 Docker 和 Docker Compose
if ! command -v docker &> /dev/null; then
    echo "错误: 需要安装 Docker"
    exit 1
fi

if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null; then
    echo "错误: 需要安装 Docker Compose"
    exit 1
fi

# 使用 docker compose 或 docker-compose
if docker compose version &> /dev/null; then
    DOCKER_COMPOSE="docker compose"
else
    DOCKER_COMPOSE="docker-compose"
fi

echo "使用: $DOCKER_COMPOSE"
echo ""

# 启动 PostgreSQL 容器
echo -e "${BLUE}启动 PostgreSQL 容器...${NC}"
$DOCKER_COMPOSE up -d postgres

echo -e "${GREEN}✓ PostgreSQL 容器已启动${NC}"
echo ""

# 等待 PostgreSQL 就绪
echo "等待 PostgreSQL 就绪..."
for i in {1..30}; do
    if docker exec ops-system-postgres pg_isready -U postgres > /dev/null 2>&1; then
        echo -e "${GREEN}✓ PostgreSQL 就绪${NC}"
        break
    fi
    echo "  等待中... ($i/30)"
    sleep 1
done

# 设置环境变量
export PGUSER=postgres
export PGHOST=localhost
export TEST_DATABASE_URL="postgresql://postgres:postgres@localhost:5432/ops_system_test"

echo ""
echo "运行测试..."
echo ""

# 运行测试
cargo test --verbose -- --test-threads=1

echo ""
echo "清理容器..."
$DOCKER_COMPOSE down

echo ""
echo -e "${GREEN}✓ Docker 测试完成！${NC}"
