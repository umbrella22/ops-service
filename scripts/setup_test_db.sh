#!/bin/bash
# PostgreSQL 测试数据库设置脚本

set -e

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}🔧 运维系统测试数据库设置${NC}"
echo "================================"
echo ""

# 检测当前用户
CURRENT_USER=$(whoami)

echo -e "${YELLOW}当前用户: $CURRENT_USER${NC}"
echo ""

# 询问使用哪个 PostgreSQL 用户
echo "请选择 PostgreSQL 认证方式:"
echo "1) 使用 postgres 用户 (需要 sudo)"
echo "2) 使用当前用户 ($CURRENT_USER)"
echo "3) 自定义"
echo ""
read -p "请输入选择 [1-3]: " choice

case $choice in
    1)
        PGUSER="postgres"
        ;;
    2)
        PGUSER="$CURRENT_USER"
        # 检查当前用户是否有 PostgreSQL 角色
        echo ""
        echo "检查当前用户是否有 PostgreSQL 角色..."
        if sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='$CURRENT_USER'" | grep -q 1; then
            echo -e "${GREEN}✓ 角色 $CURRENT_USER 已存在${NC}"
        else
            echo -e "${YELLOW}! 创建角色 $CURRENT_USER...${NC}"
            sudo -u postgres createuser --superuser $CURRENT_USER
            echo -e "${GREEN}✓ 角色 $CURRENT_USER 创建成功${NC}"
        fi
        # 设置密码
        echo ""
        read -sp "请为 $CURRENT_USER 设置 PostgreSQL 密码: " password
        echo ""
        sudo -u postgres psql -c "ALTER USER $CURRENT_USER PASSWORD '$password';"
        TEST_DATABASE_URL="postgresql://$CURRENT_USER:$password@localhost:5432/ops_system_test"
        ;;
    3)
        read -p "请输入 PostgreSQL 用户名: " PGUSER
        read -sp "请输入密码: " password
        echo ""
        TEST_DATABASE_URL="postgresql://$PGUSER:$password@localhost:5432/ops_system_test"
        ;;
    *)
        echo -e "${RED}无效选择${NC}"
        exit 1
        ;;
esac

echo ""
echo "================================"
echo -e "${GREEN}✓ 设置完成!${NC}"
echo ""
echo "请将以下内容添加到你的环境变量中:"
echo ""
if [ -z "$TEST_DATABASE_URL" ]; then
    TEST_DATABASE_URL="postgresql://postgres:postgres@localhost:5432/ops_system_test"
fi
echo -e "${BLUE}export TEST_DATABASE_URL=\"$TEST_DATABASE_URL\"${NC}"
if [ "$PGUSER" != "postgres" ]; then
    echo -e "${BLUE}export PGUSER=\"$PGUSER\"${NC}"
fi
echo ""
echo "或者添加到 ~/.bashrc 或 ~/.zshrc:"
echo ""
if [ "$PGUSER" = "postgres" ]; then
    echo "echo 'export TEST_DATABASE_URL=\"$TEST_DATABASE_URL\"' >> ~/.bashrc"
else
    echo "echo 'export TEST_DATABASE_URL=\"$TEST_DATABASE_URL\"' >> ~/.bashrc"
    echo "echo 'export PGUSER=\"$PGUSER\"' >> ~/.bashrc"
fi
echo ""
echo "然后运行: source ~/.bashrc"
echo ""
echo "现在可以运行测试了:"
echo -e "${GREEN}./run_tests.sh${NC}"
