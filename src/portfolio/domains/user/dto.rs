use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateUser {
    pub email: String,
    pub name: String,
    pub password: String,
    #[serde(default = "default_headline")]
    pub headline: String,
}
fn default_headline() -> String {
    "Contributor".into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateUser {
    pub name: String,
    pub headline: String,
    pub active: bool,
    pub password: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssignRoles {
    pub role_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleInput {
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}
