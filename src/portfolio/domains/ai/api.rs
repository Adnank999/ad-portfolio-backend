use super::dto::{GenerateRequest, GenerateResponse};
use crate::{
    common::{error::AppResult, extract::ApiJson, state::AppState},
    domains::auth::Principal,
};
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    Extension, Json, Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/ai/generate", post(generate))
        .layer(DefaultBodyLimit::max(256_000))
}
async fn generate(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    ApiJson(input): ApiJson<GenerateRequest>,
) -> AppResult<Json<GenerateResponse>> {
    Ok(Json(
        state
            .ai
            .generate(&user, input, &state.config.frontend_url)
            .await?,
    ))
}
