use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Invalid(String),
    #[error("Authentication required or credentials invalid")]
    Unauthorized,
    #[error("Permission denied")]
    Forbidden,
    #[error("Resource not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("Too many authentication attempts; try again later")]
    RateLimited,
    #[error("Too many image uploads; wait before trying again")]
    UploadRateLimited,
    #[error("Too many enquiries; please wait a minute before trying again")]
    ContactRateLimited,
    #[error("Email is temporarily unavailable. Please try again shortly")]
    ContactUnavailable,
    #[error("Internal server error")]
    Internal,
    #[error("This service is not configured")]
    Unavailable,
    #[error("Shader activation is unavailable. Apply pending backend database migrations, restart the API, then retry")]
    ShaderSchemaUnavailable,
    #[error("Image storage is temporarily unavailable. Retry completion if the upload already succeeded")]
    StorageUnavailable,
    #[error("Upload session expired; select the image again")]
    UploadExpired,
    #[error("AI generation is not configured on the server")]
    AiUnconfigured,
    #[error("The AI provider is unavailable. Please try again later")]
    AiUnavailable,
    #[error("Generation timed out. Try shorter content or retry")]
    AiTimeout,
    #[error("The generated structure was invalid or incomplete. Try a shorter request")]
    AiInvalidOutput,
    #[error("AI generation quota exceeded. Please try again later")]
    AiRateLimited,
    #[error("A generation is already running or all generation slots are busy")]
    AiBusy,
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(database) = &error {
            if database.is_unique_violation() {
                return Self::Conflict("A unique value is already in use".into());
            }
            if database.is_foreign_key_violation() {
                return Self::Invalid(
                    "A referenced record does not exist or is still in use".into(),
                );
            }
        }
        tracing::error!(error = %error, "Database operation failed");
        Self::Internal
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Invalid(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::UploadRateLimited => (StatusCode::TOO_MANY_REQUESTS, "too_many_uploads"),
            Self::ContactRateLimited => (StatusCode::TOO_MANY_REQUESTS, "too_many_enquiries"),
            Self::ContactUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "contact_unavailable"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::ShaderSchemaUnavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, "shader_schema_unavailable")
            }
            Self::StorageUnavailable => (StatusCode::BAD_GATEWAY, "storage_unavailable"),
            Self::UploadExpired => (StatusCode::GONE, "upload_expired"),
            Self::AiUnconfigured => (StatusCode::SERVICE_UNAVAILABLE, "ai_unconfigured"),
            Self::AiUnavailable => (StatusCode::BAD_GATEWAY, "ai_unavailable"),
            Self::AiTimeout => (StatusCode::GATEWAY_TIMEOUT, "ai_timeout"),
            Self::AiInvalidOutput => (StatusCode::BAD_GATEWAY, "ai_invalid_output"),
            Self::AiRateLimited => (StatusCode::TOO_MANY_REQUESTS, "ai_rate_limited"),
            Self::AiBusy => (StatusCode::TOO_MANY_REQUESTS, "ai_busy"),
        };
        let mut response = (
            status,
            Json(json!({"error": {"code": code, "message": self.to_string()}})),
        )
            .into_response();
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert("www-authenticate", "Bearer".parse().expect("static header"));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert("retry-after", "60".parse().expect("static header"));
        }
        response
    }
}

pub type AppResult<T> = Result<T, AppError>;
