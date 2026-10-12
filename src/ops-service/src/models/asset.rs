//! Asset domain models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

/// Asset group
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AssetGroup {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub environment: String,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// Create group request
#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    pub environment: String,
    pub parent_id: Option<Uuid>,
}

/// Update group request
#[derive(Debug, Deserialize)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub environment: Option<String>,
    pub parent_id: Option<Uuid>,
}

/// Host asset
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Host {
    pub id: Uuid,
    pub identifier: String,
    pub display_name: Option<String>,
    pub address: String,
    pub port: i32,
    pub group_id: Uuid,
    pub environment: String,
    pub tags: Json<Vec<String>>, // Stored as JSONB
    pub owner_id: Option<Uuid>,
    pub status: String,
    pub notes: Option<String>,
    pub os_type: Option<String>,
    pub os_version: Option<String>,
    // SSH 认证凭据（主机级，优先于全局默认值）。
    // 安全：凭据字段一律不序列化到 API 响应（此前明文私钥/口令会随
    // GET /hosts/{id} 直接下发）。凭据写入经 crypto_service 加密落盘。
    pub ssh_username: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_password: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_private_key: Option<String>,
    #[serde(skip_serializing)]
    pub ssh_key_passphrase: Option<String>,
    // SSH 主机密钥验证策略（新增）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_key_verification: Option<String>, // 存储为 "strict", "accept", "disabled"
    // SSH known_hosts（新增，JSON 格式存储）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known_hosts: Option<Json<std::collections::HashMap<String, String>>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub version: i32,
}

/// Create host request
#[derive(Debug, Deserialize)]
pub struct CreateHostRequest {
    // 校验说明见 validate()：identifier/address 长度、port 范围
    // （此前超长输入直达 DB 报 500，port 可为 0/负数）
    pub identifier: String,
    pub display_name: Option<String>,
    pub address: String,
    #[serde(default = "default_port")]
    pub port: i32,
    pub group_id: Uuid,
    pub environment: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub owner_id: Option<Uuid>,
    #[serde(default = "default_status")]
    pub status: String,
    pub notes: Option<String>,
    pub os_type: Option<String>,
    pub os_version: Option<String>,
    // SSH 认证凭据（可选）
    pub ssh_username: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_private_key: Option<String>,
    pub ssh_key_passphrase: Option<String>,
    // SSH 主机密钥验证策略（可选）
    #[serde(default)]
    pub host_key_verification: Option<String>,
    // SSH known_hosts（可选，JSON 格式）
    pub known_hosts: Option<std::collections::HashMap<String, String>>,
}

impl CreateHostRequest {
    /// 请求字段基础校验（400 语义；此前非法值直达 DB 约束抛 500）
    pub fn validate(&self) -> Result<(), String> {
        if self.identifier.is_empty() || self.identifier.len() > 255 {
            return Err("identifier must be 1-255 characters".to_string());
        }
        if self.address.is_empty() || self.address.len() > 255 {
            return Err("address must be 1-255 characters".to_string());
        }
        if !(1..=65535).contains(&self.port) {
            return Err("port must be between 1 and 65535".to_string());
        }
        if let Some(name) = &self.display_name
            && name.len() > 255
        {
            return Err("display_name must be at most 255 characters".to_string());
        }
        Ok(())
    }
}

fn default_port() -> i32 {
    22
}
fn default_status() -> String {
    "active".to_string()
}

/// Update host request
#[derive(Debug, Deserialize)]
pub struct UpdateHostRequest {
    pub display_name: Option<String>,
    pub address: Option<String>,
    pub port: Option<i32>,
    pub group_id: Option<Uuid>,
    pub environment: Option<String>,
    pub tags: Option<Vec<String>>,
    pub owner_id: Option<Uuid>,
    pub status: Option<String>,
    pub notes: Option<String>,
    pub os_type: Option<String>,
    pub os_version: Option<String>,
    // SSH 认证凭据（可选）
    pub ssh_username: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_private_key: Option<String>,
    pub ssh_key_passphrase: Option<String>,
    // SSH 主机密钥验证策略（可选）
    pub host_key_verification: Option<String>,
    // SSH known_hosts（可选，JSON 格式）
    pub known_hosts: Option<std::collections::HashMap<String, String>>,
    pub version: i32, // For optimistic locking
}

/// Host list filters
#[derive(Debug, Deserialize)]
pub struct HostListFilters {
    pub group_id: Option<Uuid>,
    pub environment: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub search: Option<String>, // Search in identifier/display_name
    /// 作用域过滤：允许的环境列表（None = 不限制；Some(空) = 无权访问任何环境）
    #[serde(skip)]
    pub allowed_environments: Option<Vec<String>>,
    /// 作用域过滤：允许的分组 ID 列表（None = 不限制；Some(空) = 无权访问任何分组）
    #[serde(skip)]
    pub allowed_groups: Option<Vec<Uuid>>,
}

/// Host response with group details
#[derive(Debug, Serialize)]
pub struct HostResponse {
    #[serde(flatten)]
    pub host: Host,
    pub group_name: String,
    pub owner_name: Option<String>,
}
