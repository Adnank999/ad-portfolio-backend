use super::{
    dto::{AssignRoles, CreateUser, RoleInput, UpdateUser, UserQuery},
    repository, service,
};
use crate::{
    common::{
        error::{AppError, AppResult},
        extract::{ApiJson, ApiQuery},
        state::AppState,
    },
    domains::auth::Principal,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", put(update_user))
        .route("/users/{id}/roles", put(assign_roles))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", put(update_role).delete(delete_role))
        .route("/permissions", get(permissions))
}
async fn list_users(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    ApiQuery(query): ApiQuery<UserQuery>,
) -> AppResult<Json<Value>> {
    user.require("users:read")?;
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);
    if !(1..=1_000_000).contains(&page) || !(1..=100).contains(&per_page) {
        return Err(AppError::Invalid("Invalid pagination".into()));
    }
    Ok(Json(
        repository::list_users(&state.pool, page, per_page).await?,
    ))
}
async fn create_user(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    ApiJson(input): ApiJson<CreateUser>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require("users:write")?;
    Ok((
        StatusCode::CREATED,
        Json(service::create_user(&state, input).await?),
    ))
}
async fn update_user(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
    ApiJson(input): ApiJson<UpdateUser>,
) -> AppResult<Json<Value>> {
    user.require("users:write")?;
    Ok(Json(service::update_user(&state, id, input).await?))
}
async fn assign_roles(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
    ApiJson(input): ApiJson<AssignRoles>,
) -> AppResult<Json<Value>> {
    user.require("roles:write")?;
    Ok(Json(service::assign_roles(&state, id, input).await?))
}
async fn list_roles(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Json<Value>> {
    user.require("roles:read")?;
    Ok(Json(repository::list_roles(&state.pool).await?))
}
async fn create_role(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    ApiJson(input): ApiJson<RoleInput>,
) -> AppResult<(StatusCode, Json<Value>)> {
    user.require("roles:write")?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id":service::save_role(&state,None,input).await?})),
    ))
}
async fn update_role(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
    ApiJson(input): ApiJson<RoleInput>,
) -> AppResult<Json<Value>> {
    user.require("roles:write")?;
    Ok(Json(
        json!({"id":service::save_role(&state,Some(id),input).await?}),
    ))
}
async fn delete_role(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    user.require("roles:write")?;
    service::delete_role(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn permissions(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<Json<Value>> {
    user.require("roles:read")?;
    let entries: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('name',name,'description',description) FROM permissions ORDER BY name").fetch_all(&state.pool).await?;
    Ok(Json(json!({"data":entries})))
}
