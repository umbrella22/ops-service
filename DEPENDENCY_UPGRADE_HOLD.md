# 依赖升级暂缓记录

本文档记录当前暂缓升级的依赖，包括当前版本、目标版本、暂缓原因和相关影响。

---

## 当前状态（2026-10-11）

**无暂缓项。** 全部直接依赖已升级至 crates.io 最新版本（`cargo outdated` 确认 "All dependencies are up to date"）。

Toolchain 基线：Rust 1.99（MSRV），edition 2024。

---

## 已完成的历史升级

### 1. jsonwebtoken（9.3 → 11.1.0）

原计划暂缓于 9.3，观望 v10 稳定性。实际迁移路径：

- **9.3 → 10.x**：按要求选择加密后端 feature（本项目使用 `rust_crypto`），`EncodingKey`/`DecodingKey` 构造方式适配。
- **10.x → 11.1.0**：API 完全兼容，零代码改动。

认证流程回归：`cargo test -p ops-service --test jwt_tests`（17 个用例）全部通过。

### 2. lapin（2.3 → 4.5.0）

原计划暂缓于 2.3。已跨多个大版本完成迁移，Runner 与控制面的 AMQP 通信（任务消费、状态/日志回传）工作正常。

---

## 升级注意事项（非暂缓，仅记录）

### sqlx 0.9+ 的动态 SQL 审计机制

sqlx 0.9 起，`sqlx::query()` / `sqlx::query_as()` 只接受 `&'static str`；动态拼接的 SQL
必须经人工审计后用 `AssertSqlSafe(...)` 显式包装，否则编译失败。

**约定**：新增动态 SQL 时，只允许拼接固定的列名/子句模板与 `$N` 占位符编号，
所有用户可控的值一律通过 `.bind()` 参数化；标识符（如动态库名）必须经过
`quote_ident` 类转义。审计通过后才可包 `AssertSqlSafe`。

### russh 传递依赖的 rc 版本

russh 0.64 的依赖树锁定了 RustCrypto 系列的 rc 预发布版本
（`crypto-bigint 0.7.0-rc`、`ecdsa 0.17.0-rc` 等）。这是上游的主动选择，
不可也不必在本项目层面干预；升级 russh 时留意即可。

---

## 更新记录

| 日期 | 操作 | 操作人 |
|------|------|--------|
| 2026-01-09 | 初始创建，记录 jsonwebtoken 和 lapin 暂缓原因 | Claude |
| 2026-10-11 | 依赖全量升级至最新；jsonwebtoken 11.1.0 与 lapin 4.5.0 迁移完成，历史暂缓项清零 | ZCode |

---

*本文档应在依赖升级完成或策略变更时及时更新。*
