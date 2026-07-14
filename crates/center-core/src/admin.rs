use serde::{Deserialize, Serialize};

use crate::CoreResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub created_at: i64,
    pub role_ids: Vec<i64>,
    pub role_names: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
    pub display_name: String,
    pub role_ids: Vec<i64>,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct UpdateUser {
    pub status: Option<String>,
    pub password: Option<String>,
    pub role_ids: Option<Vec<i64>>,
}

#[async_trait::async_trait]
pub trait UserAdmin: Send + Sync {
    async fn list_users(&self) -> CoreResult<Vec<UserRecord>>;
    async fn create_user(&self, user: CreateUser) -> CoreResult<i64>;
    async fn update_user(&self, id: i64, update: UpdateUser) -> CoreResult<()>;
    async fn delete_user(&self, id: i64) -> CoreResult<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub permission_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRole {
    pub name: String,
    pub description: String,
    pub permission_keys: Vec<String>,
}

#[async_trait::async_trait]
pub trait RoleAdmin: Send + Sync {
    async fn list_roles(&self) -> CoreResult<Vec<RoleRecord>>;
    async fn create_role(&self, role: CreateRole) -> CoreResult<i64>;
    async fn set_permissions(&self, id: i64, permission_keys: Vec<String>) -> CoreResult<()>;
    async fn delete_role(&self, id: i64) -> CoreResult<()>;
}
