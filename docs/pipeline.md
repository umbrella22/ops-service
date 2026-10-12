# Pipeline-as-code（.ops.yml）

构建作业可以不通过 API 传 steps，而是在仓库根目录放一个 `.ops.yml` 管道文件，
由 Runner 在 git clone 之后解析并执行——"管道随仓库走"，与 Drone / GitHub
Actions 的模式一致。

## 快速开始

1. 在仓库根目录创建 `.ops.yml`：

```yaml
version: 1

# 步骤级默认环境变量（创建作业时传入的 env_vars 可覆盖）
env:
  CARGO_TERM_COLOR: never

steps:
  - id: build
    name: Build
    step_type: script        # command | script | install | build | test | package | publish | 任意自定义串
    script: |
      cargo build --release
    timeout_secs: 600        # 可选，步骤超时（秒）

  - id: test
    name: Test
    step_type: command
    command: cargo test
    continue_on_failure: true   # 失败后继续执行后续步骤（整体仍判 Failed）

  - id: package
    name: Package
    step_type: command
    command: tar czf app.tar.gz target/release/app
    produces_artifact: true     # 产物收集并上传
```

2. 创建构建作业时**不传 steps**，只指定管道文件：

```bash
curl -X POST /api/v1/builds -H "Authorization: Bearer $TOKEN" -d '{
  "project_name": "my-app",
  "repository_url": "https://github.com/me/my-app",
  "branch": "main",
  "build_type": "rust",
  "pipeline_file": ".ops.yml"
}'
```

`pipeline_file` 缺省即 `.ops.yml`；可以是仓库内的任意相对路径
（如 `.ops/release.yml`），不允许 `..` 或绝对路径。

## 字段说明

| 字段 | 必填 | 说明 |
|---|---|---|
| `version` | 否 | 格式版本，当前仅 1 |
| `env` | 否 | 步骤级默认环境变量 |
| `steps[].id` | 是 | 唯一标识，仅允许字母数字与 `. _ -` |
| `steps[].name` | 否 | 展示名（缺省用 id） |
| `steps[].step_type` | 否 | 缺省 `command` |
| `steps[].command` / `script` | 二选一 | 执行内容；script 会落盘为临时脚本执行 |
| `steps[].working_dir` | 否 | 相对仓库根的工作目录 |
| `steps[].timeout_secs` | 否 | 1-86400 |
| `steps[].continue_on_failure` | 否 | 缺省 false；失败后是否继续 |
| `steps[].produces_artifact` | 否 | 缺省 false；成功后收集产物 |
| `steps[].docker_image` | 否 | 覆盖该步骤的执行镜像 |

校验失败（空步骤、重复 id、非法字符、缺 command/script）会导致构建直接
Failed，错误信息回传到作业详情。

## 与 Git 自动触发配合

配合 build triggers（`POST /api/v1/build-triggers`）可以实现 push 即构建：
触发器里 `pipeline_file` 置空即使用 `.ops.yml`，webhook 推送命中仓库+分支后
自动创建管道模式构建。详见 `handlers/build_trigger.rs` 模块注释。
