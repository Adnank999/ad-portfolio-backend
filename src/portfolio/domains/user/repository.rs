use crate::common::error::{AppError, AppResult};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

pub async fn user(pool: &PgPool, id: Uuid) -> AppResult<Value> {
    sqlx::query_scalar("SELECT jsonb_build_object('id',u.id,'email',u.email,'name',u.name,'headline',u.headline,'active',u.active,'createdAt',u.created_at,'updatedAt',u.updated_at,'roles',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',r.id,'name',r.name) ORDER BY r.name) FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE ur.user_id=u.id),'[]'::jsonb)) FROM users u WHERE id=$1")
        .bind(id).fetch_optional(pool).await?.ok_or(AppError::NotFound)
}

pub async fn list_users(pool: &PgPool, page: i64, per_page: i64) -> AppResult<Value> {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users ORDER BY created_at DESC,id LIMIT $1 OFFSET $2")
            .bind(per_page)
            .bind((page - 1) * per_page)
            .fetch_all(pool)
            .await?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await?;
    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        data.push(user(pool, id).await?);
    }
    Ok(
        json!({"data": data,"pagination":{"page":page,"perPage":per_page,"total":total,"totalPages":(total+per_page-1)/per_page}}),
    )
}

pub async fn lock_non_owner(connection: &mut PgConnection, id: Uuid) -> AppResult<()> {
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(AppError::NotFound)?;
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE ur.user_id=$1 AND r.name='owner')").bind(id).fetch_one(connection).await?;
    if owner {
        return Err(AppError::Conflict(
            "Owner accounts cannot be modified through account administration".into(),
        ));
    }
    Ok(())
}

pub async fn list_roles(pool: &PgPool) -> AppResult<Value> {
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',r.id,'name',r.name,'description',r.description,'protected',r.protected,'permissions',COALESCE((SELECT jsonb_agg(permission ORDER BY permission) FROM role_permissions WHERE role_id=r.id),'[]'::jsonb)) FROM roles r ORDER BY r.name").fetch_all(pool).await?;
    Ok(json!({"data": rows}))
}

pub async fn check_role(connection: &mut PgConnection, id: Uuid, editable: bool) -> AppResult<()> {
    let role = sqlx::query("SELECT name,protected FROM roles WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(connection)
        .await?
        .ok_or(AppError::NotFound)?;
    if role.try_get::<String, _>("name")? == "owner"
        || (editable && role.try_get::<bool, _>("protected")?)
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
