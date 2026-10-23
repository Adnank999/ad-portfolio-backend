use super::{
    dto::{AssignRoles, CreateUser, RoleInput, UpdateUser},
    repository,
};
use crate::common::{
    error::{AppError, AppResult},
    security,
    state::AppState,
    validation,
};
use serde_json::Value;
use uuid::Uuid;

pub async fn create_user(state: &AppState, input: CreateUser) -> AppResult<Value> {
    let email = validation::email(&input.email)?;
    validation::text(&input.name, "name", 1, 120)?;
    validation::text(&input.headline, "headline", 1, 120)?;
    validation::password(&input.password)?;
    let password_hash = security::hash_password_limited(state, input.password).await?;
    let id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    sqlx::query("INSERT INTO users(id,email,name,headline,password_hash) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(email)
        .bind(input.name.trim())
        .bind(input.headline.trim())
        .bind(password_hash)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='viewer'")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    repository::user(&state.pool, id).await
}

pub async fn update_user(state: &AppState, id: Uuid, input: UpdateUser) -> AppResult<Value> {
    validation::text(&input.name, "name", 1, 120)?;
    validation::text(&input.headline, "headline", 1, 120)?;
    let password_hash = if let Some(password) = input.password {
        validation::password(&password)?;
        Some(security::hash_password_limited(state, password).await?)
    } else {
        None
    };
    let mut transaction = state.pool.begin().await?;
    repository::lock_non_owner(&mut transaction, id).await?;
    sqlx::query("UPDATE users SET name=$1,headline=$2,active=$3,password_hash=COALESCE($4,password_hash),updated_at=now() WHERE id=$5").bind(input.name.trim()).bind(input.headline.trim()).bind(input.active).bind(&password_hash).bind(id).execute(&mut *transaction).await?;
    if !input.active || password_hash.is_some() {
        sqlx::query("UPDATE auth_sessions SET revoked_at=now() WHERE user_id=$1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    repository::user(&state.pool, id).await
}

pub async fn assign_roles(state: &AppState, id: Uuid, input: AssignRoles) -> AppResult<Value> {
    if input.role_ids.is_empty() || input.role_ids.len() > 20 {
        return Err(AppError::Invalid("Assign between 1 and 20 roles".into()));
    }
    let mut ids = input.role_ids;
    ids.sort();
    ids.dedup();
    let mut transaction = state.pool.begin().await?;
    repository::lock_non_owner(&mut transaction, id).await?;
    for role in &ids {
        repository::check_role(&mut transaction, *role, false).await?;
    }
    sqlx::query("DELETE FROM user_roles WHERE user_id=$1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    for role in ids {
        sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
            .bind(id)
            .bind(role)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    repository::user(&state.pool, id).await
}

pub async fn save_role(state: &AppState, id: Option<Uuid>, input: RoleInput) -> AppResult<Uuid> {
    validation::slug(&input.name)?;
    validation::text(&input.description, "description", 1, 500)?;
    if input.permissions.len() > 50
        || matches!(input.name.as_str(), "owner" | "admin" | "editor" | "viewer")
    {
        return Err(AppError::Invalid(
            "Choose a custom role name and at most 50 permissions".into(),
        ));
    }
    let mut permissions = input.permissions;
    permissions.sort();
    permissions.dedup();
    let mut transaction = state.pool.begin().await?;
    let known: Vec<String> = sqlx::query_scalar("SELECT name FROM permissions")
        .fetch_all(&mut *transaction)
        .await?;
    if permissions
        .iter()
        .any(|permission| !known.contains(permission))
    {
        return Err(AppError::Invalid("Unknown permission".into()));
    }
    let role_id = id.unwrap_or_else(Uuid::new_v4);
    if let Some(id) = id {
        repository::check_role(&mut transaction, id, true).await?;
        sqlx::query("UPDATE roles SET name=$1,description=$2 WHERE id=$3")
            .bind(input.name)
            .bind(input.description.trim())
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM role_permissions WHERE role_id=$1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
    } else {
        sqlx::query("INSERT INTO roles(id,name,description) VALUES($1,$2,$3)")
            .bind(role_id)
            .bind(input.name)
            .bind(input.description.trim())
            .execute(&mut *transaction)
            .await?;
    }
    for permission in permissions {
        sqlx::query("INSERT INTO role_permissions(role_id,permission) VALUES($1,$2)")
            .bind(role_id)
            .bind(permission)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(role_id)
}

pub async fn delete_role(state: &AppState, id: Uuid) -> AppResult<()> {
    let mut transaction = state.pool.begin().await?;
    repository::check_role(&mut transaction, id, true).await?;
    sqlx::query("DELETE FROM roles WHERE id=$1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
