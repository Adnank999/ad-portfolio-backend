use super::{
    dto::{ChangePassword, ForgotPassword, Login, Refresh, Register, ResetPassword, Tokens},
    repository, service, Principal,
};
use crate::common::{
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub fn routes(state: AppState) -> Router<AppState> {
    let public = Router::new()
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/register", post(register))
        .route("/forgot-password", post(forgot_password))
        .route("/reset-password", post(reset_password))
        .route_layer(middleware::from_fn_with_state(state.clone(), throttle));
    let private = Router::new()
        .route("/me", get(me))
        .route("/logout", post(logout))
        .route("/change-password", post(change_password))
        .route_layer(middleware::from_fn_with_state(state, require_auth));
    public.merge(private)
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> AppResult<Response> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized)?;
    let user = repository::authenticate(&state.pool, token).await?;
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

async fn throttle(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> AppResult<Response> {
    let address = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|value| value.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    state.check_login_limit(address)?;
    Ok(next.run(request).await)
}

async fn login(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<Login>,
) -> AppResult<Json<Tokens>> {
    Ok(Json(service::login(&state, input).await?))
}
async fn refresh(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<Refresh>,
) -> AppResult<Json<Tokens>> {
    Ok(Json(service::refresh(&state, input).await?))
}
async fn register(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<Register>,
) -> AppResult<StatusCode> {
    service::register(&state, input).await?;
    Ok(StatusCode::CREATED)
}
async fn forgot_password(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ForgotPassword>,
) -> AppResult<StatusCode> {
    service::forgot_password(&state, input).await?;
    Ok(StatusCode::ACCEPTED)
}
async fn reset_password(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ResetPassword>,
) -> AppResult<StatusCode> {
    service::reset_password(&state, input).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn me(Extension(user): Extension<Principal>) -> Json<Principal> {
    Json(user)
}
async fn logout(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
) -> AppResult<StatusCode> {
    sqlx::query("UPDATE auth_sessions SET revoked_at=now() WHERE id=$1")
        .bind(user.session_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn change_password(
    State(state): State<AppState>,
    Extension(user): Extension<Principal>,
    ApiJson(input): ApiJson<ChangePassword>,
) -> AppResult<StatusCode> {
    service::change_password(&state, &user, input).await?;
    Ok(StatusCode::NO_CONTENT)
}
