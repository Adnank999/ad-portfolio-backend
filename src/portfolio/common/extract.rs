use super::error::{AppError, AppResult};
use axum::{
    extract::{FromRequest, FromRequestParts, Query, Request},
    http::request::Parts,
    Json,
};
use serde::de::DeserializeOwned;

pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequest<S> for ApiJson<T> {
    type Rejection = AppError;
    async fn from_request(request: Request, state: &S) -> AppResult<Self> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_| {
                AppError::Invalid(
                    "Request must contain valid JSON matching the documented schema".into(),
                )
            })
    }
}

pub struct ApiQuery<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &S) -> AppResult<Self> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| AppError::Invalid("Invalid query parameters".into()))
    }
}
