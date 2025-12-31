# ========== 构建阶段 ==========
FROM rust:1.75.0-bookworm AS builder

# 安装依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /build

# 复制 Cargo 配置
COPY Cargo.toml Cargo.lock ./

# 创建虚拟 src 目录（利用 Docker 缓存层）
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "" > src/lib.rs

# 构建依赖（缓存层）
RUN cargo build --release && \
    rm -rf src

# 复制实际源代码
COPY src ./src
COPY migrations ./migrations

# 构建应用
RUN touch src/main.rs && \
    cargo build --release

# ========== 运行阶段 ==========
FROM debian:bookworm-slim AS runtime

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN groupadd -r opsuser && \
    useradd -r -g opsuser opsuser

# 创建目录
RUN mkdir -p /app/migrations && \
    chown -R opsuser:opsuser /app

# 设置工作目录
WORKDIR /app

# 从构建阶段复制二进制文件和迁移脚本
COPY --from=builder /build/target/release/ops-system /app/
COPY --from=builder /build/migrations /app/migrations/

# 切换到非 root 用户
USER opsuser

# 健康检查
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# 暴露端口（仅内网）
EXPOSE 3000

# 启动应用
ENTRYPOINT ["/app/ops-system"]
