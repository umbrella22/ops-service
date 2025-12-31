#!/bin/bash
# Docker 镜像构建脚本 - 支持多架构
# 此脚本会自动检测当前平台的架构并构建对应的 Docker 镜像

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
IMAGE_NAME="${IMAGE_NAME:-{{BINARY_NAME}}}"
VERSION="${VERSION:-{{VERSION}}}}"
REGISTRY="${REGISTRY:-}"

# 根据平台目录名确定架构
CURRENT_DIR=$(basename "$(pwd)")
if [[ "$CURRENT_DIR" == "linux-x86_64" ]]; then
    ARCH="amd64"
    PLATFORM="linux/amd64"
elif [[ "$CURRENT_DIR" == "linux-arm64" ]]; then
    ARCH="arm64"
    PLATFORM="linux/arm64"
else
    echo -e "${RED}错误: 无法从目录名确定架构: $CURRENT_DIR${NC}"
    echo "期望的目录名: linux-x86_64 或 linux-arm64"
    exit 1
fi

echo -e "${GREEN}=== Docker 镜像构建脚本 ===${NC}"
echo "镜像名称: $IMAGE_NAME"
echo "版本: $VERSION"
echo "架构: $ARCH ($PLATFORM)"
echo ""

# 函数：构建镜像
build_image() {
    local tag="${REGISTRY}${IMAGE_NAME}:${VERSION}-${ARCH}"
    local latest_tag="${REGISTRY}${IMAGE_NAME}:latest-${ARCH}"

    echo -e "${YELLOW}正在构建镜像: $tag${NC}"

    docker build \
        --platform "$PLATFORM" \
        --build-arg "TARGETPLATFORM=$PLATFORM" \
        --build-arg "TARGETARCH=$ARCH" \
        -t "$tag" \
        -t "$latest_tag" \
        -f Dockerfile \
        ..

    echo -e "${GREEN}✓ 镜像构建成功: $tag${NC}"
    echo ""
}

# 函数：推送镜像
push_image() {
    if [[ -n "$REGISTRY" ]]; then
        local tag="${REGISTRY}${IMAGE_NAME}:${VERSION}-${ARCH}"
        local latest_tag="${REGISTRY}${IMAGE_NAME}:latest-${ARCH}"

        echo -e "${YELLOW}正在推送镜像到仓库...${NC}"

        docker push "$tag"
        docker push "$latest_tag"

        echo -e "${GREEN}✓ 镜像推送成功${NC}"
        echo ""
    else
        echo -e "${YELLOW}跳过推送（未设置 REGISTRY 环境变量）${NC}"
    fi
}

# 主流程
main() {
    # 解析参数
    PUSH=false

    while [[ $# -gt 0 ]]; do
        case $1 in
            --push)
                PUSH=true
                shift
                ;;
            --help|-h)
                echo "用法: $0 [选项]"
                echo ""
                echo "选项:"
                echo "  --push          构建后推送镜像"
                echo "  --help, -h      显示此帮助信息"
                echo ""
                echo "环境变量:"
                echo "  IMAGE_NAME      镜像名称 (默认: {{BINARY_NAME}})"
                echo "  VERSION         版本号 (默认: {{VERSION}})"
                echo "  REGISTRY        镜像仓库地址 (例如: registry.example.com/)"
                echo ""
                echo "示例:"
                echo "  # 构建镜像"
                echo "  cd build/linux-x86_64/docker && ./build.sh"
                echo ""
                echo "  # 构建并推送镜像"
                echo "  REGISTRY=registry.example.com/ VERSION=1.0.0 ./build.sh --push"
                exit 0
                ;;
            *)
                echo -e "${RED}未知参数: $1${NC}"
                echo "使用 --help 查看帮助信息"
                exit 1
                ;;
        esac
    done

    # 构建镜像
    build_image

    # 推送镜像（如果指定）
    if [[ "$PUSH" == "true" ]]; then
        push_image
    fi

    echo -e "${GREEN}=== 构建完成 ===${NC}"
    echo ""
    echo "使用方法:"
    echo "  docker run -d -p 3000:3000 ${REGISTRY}${IMAGE_NAME}:${VERSION}-${ARCH}"
}

main "$@"
