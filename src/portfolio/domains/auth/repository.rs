use super::Principal;
use crate::common::{
    error::{AppError, AppResult},
    security,
};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub async fn authenticate(pool: &PgPool, token: &str) -> AppResult<Principal> {
    let hash = security::token_hash(token)?;
    let row = sqlx::query("SELECT u.id,u.email,u.name,u.headline,s.id AS session_id FROM auth_sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.access_expires_at>now() AND s.refresh_expires_at>now() AND u.active")
        .bind(hash).fetch_optional(pool).await?.ok_or(AppError::Unauthorized)?;
    let mut connection = pool.acquire().await?;
    principal(
        &mut connection,
        row.try_get("id")?,
        row.try_get("session_id")?,
        row.try_get("email")?,
        row.try_get("name")?,
        row.try_get("headline")?,
    )
    .await
}

pub async fn principal(
    connection: &mut PgConnection,
    id: Uuid,
    session_id: Uuid,
    email: String,
    name: String,
    headline: String,
) -> AppResult<Principal> {
    let roles = sqlx::query_scalar("SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1 ORDER BY r.name").bind(id).fetch_all(&mut *connection).await?;
    let permissions = sqlx::query_scalar("SELECT DISTINCT rp.permission FROM role_permissions rp JOIN user_roles ur ON ur.role_id=rp.role_id WHERE ur.user_id=$1 ORDER BY rp.permission").bind(id).fetch_all(connection).await?;
    Ok(Principal {
        id,
        session_id,
        email,
        name,
        headline,
        roles,
        permissions,
    })
}
