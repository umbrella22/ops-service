#!/usr/bin/env bash
# ====================================================================
# 端到端部署测试（对应 DEPLOYMENT_REPORT.md 的手工部署流程）
#
# 在独立容器（PostgreSQL + RabbitMQ）上启动 ops-service 与 ops-runner，
# 走完 注册 → 创建构建 → 容器执行 → 状态/日志/产物回传 → 运行中取消
# → 重试 的完整链路，并对数据库中的落库结果做断言。
#
# 用法:
#   make e2e                     # 默认 Docker 执行模式（真实容器路径）
#   E2E_EXEC=native make e2e     # native 执行模式（跳过镜像拉取，更快）
#   E2E_KEEP=1 make e2e          # 失败时保留容器现场便于排查
#
# 说明:
#   - 使用独立端口(35432/35672/38000)与容器名(ops-e2e-*)，不影响开发环境
#   - 首次运行会拉取 rabbitmq:3-alpine / postgres:16-alpine / ubuntu:22.04
# ====================================================================
set -euo pipefail

# ---------- 配置 ----------
E2E_EXEC="${E2E_EXEC:-docker}"          # docker | native
E2E_KEEP="${E2E_KEEP:-0}"
PG_CONTAINER="ops-e2e-postgres"
MQ_CONTAINER="ops-e2e-rabbitmq"
PG_PORT=35432
AMQP_PORT=35672
HTTP_PORT=38000
NOTIFY_PORT=39999
BASE_URL="http://localhost:${HTTP_PORT}"
DB_URL="postgresql://e2e:e2e@localhost:${PG_PORT}/ops_service_e2e"
AMQP_URL="amqp://guest:guest@localhost:${AMQP_PORT}/%2F"
RUNNER_NAME="e2e-runner-01"
ADMIN_USER="admin"
ADMIN_PASS="Admin123!" # 种子账户（migrations/000002/000003）
BUILD_TIMEOUT=300      # 含首次镜像拉取的时间余量
CANCEL_TIMEOUT=60

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; NC='\033[0m'
PASS_COUNT=0
WORK_DIR="$(mktemp -d /tmp/ops-e2e-XXXXXX)"
LOG_DIR="${WORK_DIR}/logs"
mkdir -p "$LOG_DIR"
SERVICE_LOG="${LOG_DIR}/service.log"
RUNNER_LOG="${LOG_DIR}/runner.log"
SERVICE_PID=""; RUNNER_PID=""

log()  { echo -e "${YELLOW}[e2e]${NC} $*"; }
pass() { PASS_COUNT=$((PASS_COUNT+1)); echo -e "${GREEN}PASS:${NC} $*"; }
fail() { echo -e "${RED}FAIL:${NC} $*" >&2; dump_logs; exit 1; }

dump_logs() {
  [ -f "$SERVICE_LOG" ] && { echo "----- service.log (tail 40) -----" >&2; tail -40 "$SERVICE_LOG" >&2; }
  [ -f "$RUNNER_LOG" ] && { echo "----- runner.log (tail 40) -----" >&2; tail -40 "$RUNNER_LOG" >&2; }
  echo "工作目录（日志/产物）: ${WORK_DIR}" >&2
}

cleanup() {
  local code=$?
  # kill 后 wait 吞掉 bash 的 "Terminated" 作业通知
  [ -n "${NOTIFY_PID:-}" ] && { kill "$NOTIFY_PID" 2>/dev/null || true; }
  [ -n "$RUNNER_PID" ] && { kill "$RUNNER_PID" 2>/dev/null || true; wait "$RUNNER_PID" 2>/dev/null || true; }
  [ -n "$SERVICE_PID" ] && { kill "$SERVICE_PID" 2>/dev/null || true; wait "$SERVICE_PID" 2>/dev/null || true; }
  if [ "$E2E_KEEP" = "1" ] && [ "$code" -ne 0 ]; then
    log "E2E_KEEP=1 且测试失败：保留容器现场（${PG_CONTAINER} / ${MQ_CONTAINER}）"
  else
    docker rm -f "$PG_CONTAINER" "$MQ_CONTAINER" >/dev/null 2>&1 || true
  fi
  echo ""
  echo "工作目录（日志/产物，可手动删除）: ${WORK_DIR}"
  exit "$code"
}
trap cleanup EXIT

require() {
  command -v "$1" >/dev/null 2>&1 || { echo -e "${RED}缺少依赖: $1${NC}" >&2; exit 1; }
}

# 等待直到命令成功或超时
wait_until() {
  local timeout_secs=$1 desc=$2; shift 2
  wait_until_quiet "$timeout_secs" "$@" || fail "等待超时(${timeout_secs}s): $desc"
}

# 同 wait_until 但超时不触发 fail，返回真假
wait_until_quiet() {
  local timeout_secs=$1; shift
  local deadline=$((SECONDS + timeout_secs))
  while (( SECONDS < deadline )); do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 2
  done
  return 1
}

# ---------- 0. 依赖检查 ----------
require docker
require cargo
require git
require curl
require python3

log "工作目录: ${WORK_DIR}"
log "执行模式: ${E2E_EXEC}"

# 清理上次运行残留
docker rm -f "$PG_CONTAINER" "$MQ_CONTAINER" >/dev/null 2>&1 || true

# ---------- 1. 基础设施 ----------
log "启动 PostgreSQL (${PG_CONTAINER}, :${PG_PORT})..."
docker run -d --name "$PG_CONTAINER" -p "${PG_PORT}:5432" \
  -e POSTGRES_USER=e2e -e POSTGRES_PASSWORD=e2e -e POSTGRES_DB=postgres \
  postgres:16-alpine >/dev/null
wait_until 60 "PostgreSQL 就绪" docker exec "$PG_CONTAINER" pg_isready -U e2e
pass "PostgreSQL 就绪"

# 启动 RabbitMQ。
# --user rabbitmq + tmpfs 双保险：macOS Docker Desktop 上该镜像的匿名卷
# 偶发 .erlang.cookie eacces 崩溃（间歇性），tmpfs 绕开卷、直接以 rabbitmq
# 用户运行绕开 chown 竞争；外加一次失败自动重建的自愈（实测有效）。
start_rabbitmq() {
  docker rm -f "$MQ_CONTAINER" >/dev/null 2>&1 || true
  docker run -d --name "$MQ_CONTAINER" -p "${AMQP_PORT}:5672" \
    --user rabbitmq \
    --tmpfs /var/lib/rabbitmq:rw,noexec,nosuid,size=256m \
    rabbitmq:3-alpine >/dev/null
}
rabbit_ready() { docker exec "$MQ_CONTAINER" rabbitmqctl ping; }

log "启动 RabbitMQ (${MQ_CONTAINER}, :${AMQP_PORT})..."
# Docker Desktop 快速建删循环下守护进程可能瞬时不健康（/_ping 500），
# 最多重建两次并递增等待
RABBIT_OK=0
for attempt in 1 2 3; do
  start_rabbitmq
  if wait_until_quiet $((30 + attempt * 15)) rabbit_ready; then
    RABBIT_OK=1
    break
  fi
  log "RabbitMQ 第 ${attempt} 次启动失败（Docker Desktop 卷权限/守护进程抖动），重建重试..."
done
[ "$RABBIT_OK" = "1" ] || fail "RabbitMQ 无法启动"
pass "RabbitMQ 就绪"

# ---------- 2. 编译 ----------
log "编译 ops-service / ops-runner..."
cargo build -q -p ops-service -p ops-runner || fail "编译失败"

# 通知捕获服务：记录 POST body 到文件，供通知断言使用
NOTIFY_OUT="${WORK_DIR}/notify-hook.log"
python3 - "$NOTIFY_PORT" "$NOTIFY_OUT" > /dev/null 2>&1 <<'PYNOTIFY' &
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

port, out = int(sys.argv[1]), sys.argv[2]

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get('content-length', 0))
        body = self.rfile.read(length)
        with open(out, 'ab') as f:
            f.write(body + b'\n')
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{}')

    def log_message(self, *args):
        pass

HTTPServer(('127.0.0.1', port), H).serve_forever()
PYNOTIFY
NOTIFY_PID=$!

# ---------- 3. 启动控制面 ----------
log "启动 ops-service (:${HTTP_PORT})..."
# 注意：显式覆盖 .env.development 中所有影响行为连接的变量——
# dotenv 不覆盖"已导出"的变量，但未导出的 OPS_SERVER__ADDR 等会让
# .env.development 的值生效（实测踩坑：服务绑到 3000 而脚本轮询 38000）
OPS_ENV=development \
OPS_SERVER__ADDR="0.0.0.0:${HTTP_PORT}" \
OPS_SERVER__GRACEFUL_SHUTDOWN_TIMEOUT_SECS=3 \
OPS_NOTIFICATIONS__ENABLED=true \
OPS_NOTIFICATIONS__KIND=feishu \
OPS_NOTIFICATIONS__WEBHOOK_URL="http://localhost:${NOTIFY_PORT}/hook" \
OPS_DATABASE__URL="$DB_URL" \
OPS_SECURITY__JWT_SECRET="e2e-local-secret-key-at-least-32-chars!" \
OPS_SECURITY__RUNNER_API_KEY="e2e-runner-api-key" \
OPS_SECURITY__TRUST_PROXY=false \
OPS_RABBITMQ__AMQP_URL="$AMQP_URL" \
"$PWD/target/debug/ops-service" > "$SERVICE_LOG" 2>&1 &
SERVICE_PID=$!

wait_until 60 "ops-service /health 就绪" curl -sf "${BASE_URL}/health"
grep -q "Migrations completed" "$SERVICE_LOG" || fail "迁移未完成"
pass "控制面启动，迁移完成（含自动建库）"

# ---------- 4. 启动 Runner ----------
log "启动 ops-runner (${RUNNER_NAME})..."
RUNNER_NAME="$RUNNER_NAME" \
RUNNER_CAPABILITIES=general,rust,node \
RUNNER_DOCKER_SUPPORTED=true \
RUNNER_MAX_CONCURRENT_JOBS=2 \
CONTROL_PLANE_API_URL="$BASE_URL" \
RUNNER_API_KEY="e2e-runner-api-key" \
RABBITMQ_AMQP_URL="$AMQP_URL" \
RUNNER_WORKSPACE_DIR="${WORK_DIR}/workspaces" \
ARTIFACT_STORAGE_ENDPOINT="${WORK_DIR}/artifacts" \
"$PWD/target/debug/ops-runner" > "$RUNNER_LOG" 2>&1 &
RUNNER_PID=$!

TOKEN=$(curl -sf -X POST "${BASE_URL}/api/v1/auth/login" \
  -H 'content-type: application/json' \
  -d "{\"username\":\"${ADMIN_USER}\",\"password\":\"${ADMIN_PASS}\"}" \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['access_token'])") \
  || fail "admin 登录失败"
pass "admin 登录成功"

runner_registered() {
  curl -sf "${BASE_URL}/api/v1/runners" -H "Authorization: Bearer $TOKEN" \
    | grep -q "\"name\":\"${RUNNER_NAME}\""
}
wait_until 30 "Runner 注册" runner_registered
pass "Runner 注册成功"

# 执行模式：native 时关闭控制面下发的 Docker 配置（心跳热更新即生效）
if [ "$E2E_EXEC" = "native" ]; then
  docker exec "$PG_CONTAINER" psql -U e2e -d ops_service_e2e -q \
    -c "UPDATE runner_docker_configs SET enabled = false WHERE name = 'default';"
  log "已切换为 native 执行模式（关闭 runner_docker_configs.default）"
  sleep 2
fi

# ---------- 5. 准备 demo 项目 ----------
DEMO_DIR="${WORK_DIR}/demo-project"
mkdir -p "$DEMO_DIR/src"
cat > "$DEMO_DIR/README.md" <<'EOF'
# ops-e2e-demo
EOF
cat > "$DEMO_DIR/build.sh" <<'EOF'
#!/bin/sh
set -e
echo "[build] compiling demo project..."
mkdir -p dist
echo "demo artifact built at $(date)" > dist/demo-artifact.txt
echo "[build] done"
EOF
chmod +x "$DEMO_DIR/build.sh"
cat > "$DEMO_DIR/.ops.yml" <<'EOF'
version: 1
env:
  PIPELINE_ENV: from-ops-yml
steps:
  - id: pipe-info
    name: Pipeline Info
    step_type: command
    command: echo PIPELINE_ENV=$PIPELINE_ENV && pwd
  - id: pipe-build
    name: Pipeline Build
    step_type: script
    script: sh build.sh
    timeout_secs: 60
EOF
git -C "$DEMO_DIR" init -q -b main
git -C "$DEMO_DIR" add -A
git -C "$DEMO_DIR" -c user.email=e2e@ops.local -c user.name=e2e commit -qm "initial"
DEMO_COMMIT=$(git -C "$DEMO_DIR" rev-parse --short HEAD)
log "demo 项目就绪 (commit ${DEMO_COMMIT})"

if [ "$E2E_EXEC" = "docker" ]; then
  log "预拉取 ubuntu:22.04（首次运行耗时，之后有缓存）..."
  docker pull -q ubuntu:22.04 >/dev/null 2>&1 || true
fi

create_build() { # $1 = steps JSON 数组字符串，输出 job id
  curl -sf -X POST "${BASE_URL}/api/v1/builds" \
    -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
    -d "$1" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])"
}

job_snapshot() { # $1 = job id，输出 "status|step:status(exit),..."
  curl -sf "${BASE_URL}/api/v1/builds/$1" -H "Authorization: Bearer $TOKEN" \
    | python3 -c "
import sys,json
d=json.load(sys.stdin)
steps=','.join(f\"{s['id']}:{s['status']}({s.get('exit_code')})\" for s in d.get('steps',[]))
print(f\"{d.get('status')}|{steps}\")"
}

wait_job_status() { # $1 job id, $2 期望终态(逗号分隔), $3 timeout; 成功时输出快照
  local id=$1 want=$2 timeout=$3 snap=""
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    snap=$(job_snapshot "$id" || true)
    case "$snap" in
      *failed*|*completed*|*cancelled*) break;;
    esac
    sleep 3
  done
  echo "$snap"
}

psql_job() { # 直接查库断言（最强断言面）
  docker exec "$PG_CONTAINER" psql -U e2e -d ops_service_e2e -t -A -c "$1"
}

# ---------- 6. 测试 1：完整构建流水线 ----------
log "测试 1：完整构建（创建 → 派发 → 容器执行 → 状态/日志/产物回传）"
STEPS_FULL='{"project_name":"ops-e2e-demo","repository_url":"file://'"$DEMO_DIR"'","branch":"main","build_type":"other","env_vars":{"DEMO_ENV":"e2e"},"timeout_secs":300,"steps":['\
'{"id":"info","name":"Info","step_type":"command","command":"pwd && echo DEMO_ENV=$DEMO_ENV && uname -m"},'\
'{"id":"build","name":"Build","step_type":"script","script":"sh build.sh","timeout_secs":60},'\
'{"id":"verify","name":"Verify","step_type":"command","command":"cat dist/demo-artifact.txt","produces_artifact":true}]}'
JOB1=$(create_build "$STEPS_FULL") || fail "创建构建作业失败"
log "  作业: $JOB1"

SNAP1=$(wait_job_status "$JOB1" "completed" "$BUILD_TIMEOUT")
echo "  终态: $SNAP1"
case "$SNAP1" in
  "completed|info:succeeded(0),build:succeeded(0),verify:succeeded(0)")
    pass "构建完成，三步骤全部 succeeded(0)" ;;
  *) fail "构建终态/步骤状态不符合预期: $SNAP1" ;;
esac

# 步骤日志内容落库断言（直接查 DB）
LOGCHECK=$(psql_job "SELECT COUNT(*) FROM build_steps WHERE job_id='$JOB1' AND output_detail LIKE '%compiling demo project%';")
[ "$LOGCHECK" = "1" ] || fail "步骤日志未落库（output_detail 缺少构建输出）"
pass "步骤日志已回传落库"

# 环境变量注入断言
ENVCHECK=$(psql_job "SELECT COUNT(*) FROM build_steps WHERE job_id='$JOB1' AND output_detail LIKE '%DEMO_ENV=e2e%';")
[ "$ENVCHECK" = "1" ] || fail "构建环境变量未注入到步骤执行环境"
pass "环境变量注入验证通过"

# 产物上传断言（runner 本地存储落盘）
ARTIFACT="${WORK_DIR}/artifacts/ops-e2e-demo/other/latest/demo-artifact.txt"
[ -f "$ARTIFACT" ] || fail "产物未上传: $ARTIFACT 不存在"
grep -q "demo artifact built" "$ARTIFACT" || fail "产物内容不正确"
pass "产物已上传（${ARTIFACT#$WORK_DIR/}）"

# 终态通知断言（构建完成 → 飞书格式 webhook 到本地捕获服务）
sleep 2
NOTIFY_HIT=$(grep -ac "msg_type.*text\|构建终态通知" "$NOTIFY_OUT" 2>/dev/null || echo 0)
[ "$NOTIFY_HIT" -ge 1 ] || fail "终态通知未到达 webhook（见 ${NOTIFY_OUT}）"
pass "终态通知已推送到 webhook"


# ---------- 6.5 测试 1.5：SSE 实时日志 tail ----------
# 广播通道不回放历史事件：必须先创建带慢步骤的构建（为订阅留出窗口），
# 订阅建立后再等构建推进，断言日志与步骤状态事件实时到达订阅端。
log "测试 1.5：SSE 实时日志 tail"
SSE_OUT="${WORK_DIR}/sse.out"
STEPS_SSE='{"project_name":"ops-e2e-demo","repository_url":"file://'"$DEMO_DIR"'","branch":"main","build_type":"other","timeout_secs":300,"steps":[''{"id":"sse","name":"SSE Step","step_type":"command","command":"sleep 8; echo sse-log-marker-12345"}]}'
SSE_JOB=$(create_build "$STEPS_SSE") || fail "SSE 测试构建创建失败"

# 构建的第一步带 sleep 8，订阅在步骤执行期间建立
curl -sN --max-time 60 "${BASE_URL}/api/v1/stream/builds/$SSE_JOB" \
  -H "Authorization: Bearer $TOKEN" -o "$SSE_OUT" &
SSE_PID=$!

wait_job_status "$SSE_JOB" "completed" 120 >/dev/null
sleep 2
kill "$SSE_PID" 2>/dev/null || true; wait "$SSE_PID" 2>/dev/null || true

grep -q "build_step_status_changed" "$SSE_OUT" \
  && grep -q "sse-log-marker-12345" "$SSE_OUT" \
  && pass "SSE 实时流：步骤状态事件与日志内容均实时到达订阅端" \
  || fail "SSE 实时流断言失败（见 ${SSE_OUT}）"

# ---------- 6.7 测试 1.6：Pipeline-as-code（.ops.yml） ----------
log "测试 1.6：Pipeline-as-code"
STEPS_PIPE='{"project_name":"ops-e2e-demo","repository_url":"file://'"$DEMO_DIR"'","branch":"main","build_type":"other","timeout_secs":300,"steps":[],"pipeline_file":".ops.yml"}'
PIPE_JOB=$(create_build "$STEPS_PIPE") || fail "pipeline 模式构建创建失败"
SNAP_P=$(wait_job_status "$PIPE_JOB" "completed" "$BUILD_TIMEOUT")
echo "  管道作业终态: $SNAP_P"
case "$SNAP_P" in
  "completed|pipe-build:succeeded(0),pipe-info:succeeded(0)")
    pass "pipeline 模式：.ops.yml 两步骤全部 succeeded(0)" ;;
  *) fail "pipeline 模式终态不符合预期: $SNAP_P" ;;
esac
PIPE_ENV=$(psql_job "SELECT COUNT(*) FROM build_steps WHERE job_id='$PIPE_JOB' AND output_detail LIKE '%from-ops-yml%';")
[ "$PIPE_ENV" = "1" ] || fail "管道级 env 未注入"
pass "管道级 env 注入验证通过"
# API 校验：steps 与 pipeline_file 互斥
MUTEX=$(curl -s -o /dev/null -w "%{http_code}" -X POST "${BASE_URL}/api/v1/builds" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"project_name":"x","repository_url":"file:///tmp","branch":"main","build_type":"other","steps":[{"id":"a","name":"a","step_type":"command","command":"true"}],"pipeline_file":".ops.yml"}')
[ "$MUTEX" = "400" ] || fail "steps+pipeline_file 应互斥（期望 400，得到 $MUTEX）"
pass "steps/pipeline_file 互斥校验（400）"

# ---------- 6.8 测试 1.7：Git push 自动触发 ----------
log "测试 1.7：Git push 自动触发"
TRIGGER_SECRET="e2e-trigger-secret-0123456789"
TRIG=$(curl -sf -X POST "${BASE_URL}/api/v1/build-triggers" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"name":"e2e-github","secret":"'"$TRIGGER_SECRET"'","provider":"github","repository_url":"file://'"$DEMO_DIR"'","branch":"main","project_name":"ops-e2e-demo","build_type":"other","pipeline_file":".ops.yml"}')
TRIG_ID=$(echo "$TRIG" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
[ -n "$TRIG_ID" ] || fail "触发器创建失败: $TRIG"
pass "触发器创建成功"

# 1) 错签名 → 401
PAYLOAD="{\"ref\":\"refs/heads/main\",\"after\":\"$(git -C "$DEMO_DIR" rev-parse HEAD)\"}"
BAD=$(curl -s -o /dev/null -w "%{http_code}" -X POST "${BASE_URL}/api/v1/webhooks/git/$TRIG_ID" \
  -H 'content-type: application/json' -H "X-Hub-Signature-256: sha256=deadbeef" \
  -d "$PAYLOAD")
[ "$BAD" = "401" ] || fail "错误签名应 401（得到 $BAD）"
pass "错误签名被拒绝（401）"

# 2) 正确签名（GitHub HMAC-SHA256）→ 自动创建并完成构建
SIG=$(printf '%s' "$PAYLOAD" | openssl dgst -sha256 -hmac "$TRIGGER_SECRET" | awk '{print $2}')
HOOK=$(curl -sf -X POST "${BASE_URL}/api/v1/webhooks/git/$TRIG_ID" \
  -H 'content-type: application/json' \
  -H "X-Hub-Signature-256: sha256=$SIG" -H "X-GitHub-Event: push" \
  -d "$PAYLOAD")
TRIG_JOB=$(echo "$HOOK" | python3 -c "import sys,json;print(json.load(sys.stdin).get('job_id',''))")
[ -n "$TRIG_JOB" ] || fail "webhook 未创建构建: $HOOK"
SNAP_T=$(wait_job_status "$TRIG_JOB" "completed" "$BUILD_TIMEOUT")
case "$SNAP_T" in
  completed*pipe-info:succeeded*) pass "webhook 自动触发构建完成（${SNAP_T}）" ;;
  *) fail "webhook 触发的构建终态异常: $SNAP_T" ;;
esac

# 3) 不匹配分支 → 忽略
PAYLOAD2="{\"ref\":\"refs/heads/dev\",\"after\":\"abc\"}"
SIG2=$(printf '%s' "$PAYLOAD2" | openssl dgst -sha256 -hmac "$TRIGGER_SECRET" | awk '{print $2}')
IGN=$(curl -sf -X POST "${BASE_URL}/api/v1/webhooks/git/$TRIG_ID" \
  -H 'content-type: application/json' -H "X-Hub-Signature-256: sha256=$SIG2" \
  -d "$PAYLOAD2")
echo "$IGN" | grep -q "branch not matched" || fail "不匹配分支应被忽略: $IGN"
pass "不匹配分支被忽略"

# 4) 触发列表
TLIST=$(curl -sf "${BASE_URL}/api/v1/build-triggers" -H "Authorization: Bearer $TOKEN")
echo "$TLIST" | grep -q '"name":"e2e-github"' && echo "$TLIST" | grep -q '"secret"' && fail "列表泄漏了 secret" || true
echo "$TLIST" | grep -q '"name":"e2e-github"' || fail "触发器列表缺失"
pass "触发器列表正常且不含 secret"

# ---------- 7. 测试 2：运行中取消 ----------# ---------- 7. 测试 2：运行中取消 ----------
log "测试 2：运行中取消（容器应被主动停止）"
STEPS_LONG='{"project_name":"ops-e2e-demo","repository_url":"file://'"$DEMO_DIR"'","branch":"main","build_type":"other","timeout_secs":300,"steps":['\
'{"id":"long","name":"Long","step_type":"command","command":"echo starting; sleep 120; echo should-not-appear"},'\
'{"id":"after","name":"After","step_type":"command","command":"echo after"}]}'
JOB2=$(create_build "$STEPS_LONG") || fail "创建取消测试作业失败"

wait_running() { case "$(job_snapshot "$JOB2" || true)" in running*) return 0;; *) return 1;; esac; }
wait_until 60 "作业进入 running" wait_running

CANCEL_RESP=$(curl -sf -X POST "${BASE_URL}/api/v1/builds/$JOB2/cancel" -H "Authorization: Bearer $TOKEN") \
  || fail "取消请求失败: $CANCEL_RESP"
echo "$CANCEL_RESP" | grep -q '"cancelled"' || fail "取消响应异常: $CANCEL_RESP"

# runner 侧终态回执后状态必须保持 cancelled（不被覆盖）
sleep 8
STATUS2=$(psql_job "SELECT status::text FROM build_jobs WHERE id='$JOB2';")
[ "$STATUS2" = "cancelled" ] || fail "取消后状态被覆盖: $STATUS2"
DONE2=$(psql_job "SELECT completed_at IS NOT NULL FROM build_jobs WHERE id='$JOB2';")
[ "$DONE2" = "t" ] || fail "取消后 completed_at 未设置"

# 后续步骤不得被执行
AFTER2=$(psql_job "SELECT COALESCE(MAX(CASE WHEN status='succeeded' THEN 1 ELSE 0 END),0) FROM build_steps WHERE job_id='$JOB2' AND step_id='after';")
[ "$AFTER2" = "0" ] || fail "取消后后续步骤仍被执行"
grep -q "Received build cancel: job=$JOB2" "$RUNNER_LOG" \
  || fail "runner 未收到取消消息"
grep -qE "(Container step cancelled|aborted by cancellation).*(Long|cancel)" "$RUNNER_LOG" \
  || grep -q "aborted by cancellation" "$RUNNER_LOG" \
  || fail "runner 未中止执行中的步骤"
pass "运行中取消：消息到达、容器停止、状态保持、后续步骤未执行"

# ---------- 8. 测试 3：终态重试 ----------
log "测试 3：对 cancelled 作业重试"
RETRY_RESP=$(curl -sf -X POST "${BASE_URL}/api/v1/builds/$JOB2/retry" -H "Authorization: Bearer $TOKEN") \
  || fail "重试请求失败: $RETRY_RESP"
JOB3=$(echo "$RETRY_RESP" | python3 -c "import sys,json;print(json.load(sys.stdin)['id'])")
[ -n "$JOB3" ] && [ "$JOB3" != "$JOB2" ] || fail "重试未创建新作业"

SNAP3=$(wait_job_status "$JOB3" "completed" "$BUILD_TIMEOUT")
echo "  重试作业终态: $SNAP3"
case "$SNAP3" in
  "completed|long:succeeded(0),after:succeeded(0)")
    pass "重试作业完整执行成功" ;;
  *) fail "重试作业终态不符合预期: $SNAP3" ;;
esac

# 并发额度归还断言：所有作业结束后 runner 计数应归零（心跳回写）
sleep 32  # 等一个心跳周期（默认 30s）
CJ=$(curl -sf "${BASE_URL}/api/v1/runners" -H "Authorization: Bearer $TOKEN" \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['runners'][0]['current_jobs'])")
[ "$CJ" = "0" ] || fail "Runner 并发计数未归还: current_jobs=$CJ"
pass "Runner 并发计数已归还（current_jobs=0）"

# ---------- 完成 ----------
echo ""
echo -e "${GREEN}=============================================="
echo -e "  E2E 全部通过：${PASS_COUNT} 项断言"
echo -e "==============================================${NC}"
log "日志与产物保留在: ${WORK_DIR}"
