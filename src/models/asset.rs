//! Asset domain models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    pub tags: Vec<String>, // Stored as JSONB
    pub owner_id: Option<Uuid>,
    pub status: String,
    pub notes: Option<String>,
    pub os_type: Option<String>,
    pub os_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub version: i32,
}

/// Create host request
#[derive(Debug, Deserialize)]
pub struct CreateHostRequest {
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
}

fn default_port() -> i32 { 22 }
fn default_status() -> String { "active".to_string() }

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
}

/// Host response with group details
#[derive(Debug, Serialize)]
pub struct HostResponse {
    #[serde(flatten)]
    pub host: Host,
    pub group_name: String,
    pub owner_name: Option<String>,
}
