use super::{dto::*, repository};
use crate::{
    common::{
        error::AppResult,
        extract::{ApiJson, ApiQuery},
        state::AppState,
    },
    domains::auth::Principal,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Extension, Json, Router,
};
use serde_json::Value;
use uuid::Uuid;

pub fn public_routes(kind: Kind) -> Router<AppState> {
    Router::new()
        .route(&format!("/{}", kind.plural()), get(list_public))
        .route(&format!("/{}/{{slug}}", kind.plural()), get(read_public))
        .layer(Extension(kind))
}
pub fn admin_routes(kind: Kind) -> Router<AppState> {
    Router::new()
        .route(&format!("/{}", kind.plural()), get(list_admin).post(create))
        .route(
            &format!("/{}/{{id}}", kind.plural()),
            get(read_admin).put(update).delete(delete),
        )
        .layer(Extension(kind))
}

async fn list_public(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> AppResult<Json<Value>> {
    Ok(Json(repository::list(&state, kind, query, false).await?))
}
async fn read_public(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Path(slug): Path<String>,
) -> AppResult<Response> {
    let record = repository::by_slug(&state.pool, kind, &slug).await?;
    if record.slug != slug {
        return Ok(
            Redirect::permanent(&format!("/api/v1/{}/{}", kind.plural(), record.slug))
                .into_response(),
        );
    }
    Ok(Json(repository::present(&record, &state.config, false, true)?).into_response())
}
async fn list_admin(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Extension(user): Extension<Principal>,
    ApiQuery(query): ApiQuery<ListQuery>,
) -> AppResult<Json<Value>> {
    user.require("content:read")?;
    Ok(Json(repository::list(&state, kind, query, true).await?))
}
async fn read_admin(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    user.require("content:read")?;
    Ok(Json(repository::present(
        &repository::by_id(&state.pool, kind, id).await?,
        &state.config,
        true,
        true,
    )?))
}
async fn create(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Extension(user): Extension<Principal>,
    ApiJson(input): ApiJson<ContentWrite>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let record = repository::save(&state, kind, None, &user, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(repository::present(&record, &state.config, true, true)?),
    ))
}
async fn update(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
    ApiJson(input): ApiJson<ContentWrite>,
) -> AppResult<Json<Value>> {
    let record = repository::save(&state, kind, Some(id), &user, input).await?;
    Ok(Json(repository::present(
        &record,
        &state.config,
        true,
        true,
    )?))
}
async fn delete(
    State(state): State<AppState>,
    Extension(kind): Extension<Kind>,
    Extension(user): Extension<Principal>,
    Path(id): Path<Uuid>,
    ApiQuery(query): ApiQuery<VersionQuery>,
) -> AppResult<StatusCode> {
    user.require(&kind.permission("delete"))?;
    repository::delete(&state, kind, id, query.version).await?;
    Ok(StatusCode::NO_CONTENT)
}
