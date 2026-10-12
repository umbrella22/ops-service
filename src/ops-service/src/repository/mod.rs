//! Database repository layer

pub mod asset_repo;
pub mod audit_repo;
pub mod auth_repo;
pub mod role_repo;
pub mod user_repo;

pub use asset_repo::*;
pub use audit_repo::*;
pub use auth_repo::*;
pub use role_repo::*;
pub use user_repo::*;

/// 转义 LIKE/ILIKE 模式中的通配符（配 `ESCAPE '\'` 使用）。
/// 用户输入直接拼进 `%...%` 时，`%`/`_`/`\` 会被当作通配符——
/// 攻击者可用 `%%%` 触发全表扫描，或用 `%` 枚举数据。
pub(crate) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
