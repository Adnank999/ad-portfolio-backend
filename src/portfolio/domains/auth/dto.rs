use super::Principal;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Login {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Register {
    pub email: String,
    pub name: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForgotPassword {
    pub email: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetPassword {
    pub token: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Refresh {
    pub refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub user: Principal,
}
