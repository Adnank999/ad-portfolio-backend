use super::service::{Asset, PresignInput, PresignResponse};
use crate::{
    common::{error::AppResult, extract::ApiJson, state::AppState},
    domains::auth::Principal,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/uploads/policy", get(policy))
        .route("/uploads/presign", post(presign))
        .route("/uploads/{id}/complete", post(complete))
        .route("/uploads/{id}", axum::routing::delete(abandon))
}

fn authorize(actor: &Principal) -> AppResult<()> {
    if [
        "blogs:write",
        "projects:write",
        "case-studies:write",
        "services:write",
    ]
    .iter()
    .any(|permission| actor.permissions.iter().any(|owned| owned == permission))
    {
        Ok(())
    } else {
        Err(crate::common::error::AppError::Forbidden)
    }
}

async fn policy(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
) -> AppResult<Json<Value>> {
    authorize(&actor)?;
    Ok(Json(json!({"enabled":state.media.config.is_some()})))
}
async fn presign(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    ApiJson(input): ApiJson<PresignInput>,
) -> AppResult<(StatusCode, Json<PresignResponse>)> {
    Ok((
        StatusCode::CREATED,
        Json(state.media.presign(&state.pool, &actor, input).await?),
    ))
}
async fn complete(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Asset>> {
    authorize(&actor)?;
    let purpose: String =
        sqlx::query_scalar("SELECT purpose FROM image_uploads WHERE id=$1 AND requested_by=$2")
            .bind(id)
            .bind(actor.id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(crate::common::error::AppError::NotFound)?;
    let purpose: super::policy::Purpose = serde_json::from_value(Value::String(purpose))
        .map_err(|_| crate::common::error::AppError::Internal)?;
    actor.require(&format!("{}:write", purpose.kind()))?;
    Ok(Json(state.media.complete(&state.pool, id, actor.id).await?))
}
async fn abandon(
    State(state): State<AppState>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    authorize(&actor)?;
    state.media.abandon(&state.pool, id, actor.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
