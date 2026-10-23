pub mod api;
pub mod dto;
pub mod repository;
pub mod service;

use crate::common::error::{AppError, AppResult};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub id: Uuid,
    pub session_id: Uuid,
    pub email: String,
    pub name: String,
    pub headline: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

impl Principal {
    pub fn require(&self, permission: &str) -> AppResult<()> {
        if self.permissions.iter().any(|entry| entry == permission) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}
