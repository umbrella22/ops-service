.PHONY: help test test-unit test-integration test-all clean-db setup-db setup-env fmt clippy build coverage ci docker-test docker-up docker-down package package-all package-x86_64 package-arm64 package-clean package-validate dist-x86_64 dist-arm64 dist-all

help:
	@echo "运维系统 - 测试命令"
	@echo ""
	@echo "可用命令:"
	@echo "  make test-all           - 运行所有测试"
	@echo "  make test-unit          - 运行单元测试"
	@echo "  make test-integration   - 运行集成测试"
	@echo "  make clean-db           - 清理测试数据库"
	@echo "  make setup-db           - 设置测试数据库"
	@echo "  make setup-env          - 交互式设置环境"
	@echo "  make fmt                - 格式化代码"
	@echo "  make clippy             - 运行 clippy 检查"
	@echo "  make build              - 构建项目"
	@echo "  make coverage           - 生成代码覆盖率报告"
	@echo "  make ci                 - 运行完整 CI 检查"
	@echo "  make docker-test        - Docker 测试"
	@echo "  make docker-up          - 启动 Docker 环境"
	@echo "  make docker-down        - 停止 Docker 环境"
	@echo ""
	@echo "构建与打包命令:"
	@echo "  make package            - 创建当前平台包 (x86_64)"
	@echo "  make package-all        - 创建所有平台包"
	@echo "  make package-x86_64     - 创建 Linux x86_64 包"
	@echo "  make package-arm64      - 创建 Linux ARM64 包"
	@echo "  make package-validate   - 验证包内容"
	@echo "  make package-clean      - 清理构建目录"
	@echo "  make dist-all           - 创建所有平台的发布包"
	@echo "  make dist-x86_64        - 创建 x86_64 发布包"
	@echo "  make dist-arm64         - 创建 ARM64 发布包"
	@echo ""

test-all:
	@echo "🧪 运行所有测试..."
	@./scripts/run_tests.sh

test-unit:
	@echo "🧪 运行单元测试..."
	@cargo test --lib -- --test-threads=1

test-integration:
	@echo "🧪 运行集成测试..."
	@./scripts/run_tests.sh

test-watch:
	@echo "🔍 监视模式: 文件变化时自动运行测试"
	@cargo watch -x test

setup-db:
	@echo "📊 设置测试数据库..."
	@createdb ops_system_test 2>/dev/null || echo "数据库已存在"
	@echo "✓ 测试数据库就绪"

clean-db:
	@echo "🧹 清理测试数据库..."
	@dropdb ops_system_test || true
	@echo "✓ 测试数据库已清理"

setup-env:
	@echo "🔧 设置测试环境..."
	@./scripts/setup_test_db.sh

ci: fmt clippy test-all

fmt:
	@echo "🎨 格式化代码..."
	@cargo fmt

clippy:
	@echo "🔍 运行 Clippy 检查..."
	@cargo clippy -- -D warnings

build:
	@echo "🔨 构建项目..."
	@cargo build --release

coverage:
	@echo "📊 生成代码覆盖率报告..."
	@cargo tarpaulin --out Html --output-dir coverage
	@echo "✓ 覆盖率报告已生成: coverage/index.html"

ci: fmt clippy test-all
	@echo "✅ CI 检查完成!"

docker-test:
	@echo "🐳 运行 Docker 测试..."
	@./scripts/test_docker.sh

docker-up:
	@echo "🐳 启动 Docker 环境..."
	@docker compose -f docker-compose.test.yml up -d
	@echo "✓ Docker 环境已启动"
	@echo "数据库: postgresql://postgres:postgres@localhost:5432/ops_system_test"

docker-down:
	@echo "🐳 停止 Docker 环境..."
	@docker compose -f docker-compose.test.yml down
	@echo "✓ Docker 环境已停止"

# ========== 构建与打包 ==========

package: package-x86_64
	@echo "✓ 包已创建"

package-all: package-x86_64 package-arm64
	@echo "✓ 所有平台的包已创建"

package-x86_64:
	@echo "📦 正在创建 Linux x86_64 包..."
	@./scripts/build/package.sh x86_64

package-arm64:
	@echo "📦 正在创建 Linux ARM64 包..."
	@./scripts/build/package.sh arm64

package-validate:
	@echo "🔍 验证包内容..."
	@./scripts/build/validate.sh

package-clean:
	@echo "🧹 清理构建目录..."
	@rm -rf build/
	@echo "✓ 构建目录已清理"

# ========== 发布包 ==========

dist-all: dist-x86_64 dist-arm64
	@echo "✓ 所有发布包已创建"

dist-x86_64: package-x86_64
	@echo "📦 正在创建 x86_64 发布归档..."
	@./scripts/build/dist.sh x86_64

dist-arm64: package-arm64
	@echo "📦 正在创建 ARM64 发布归档..."
	@./scripts/build/dist.sh arm64
