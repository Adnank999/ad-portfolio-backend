use crate::{
    common::{
        error::{AppError, AppResult},
        state::AppState,
    },
    domains::{
        ai, auth,
        content::{self, dto::Kind},
        user,
    },
};
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::time::Duration;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

pub fn create_router(state: AppState) -> AppResult<Router> {
    let origins = state
        .config
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse::<HeaderValue>()
                .map_err(|_| AppError::Internal)
        })
        .collect::<AppResult<Vec<_>>>()?;
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);
    let admin = user::api::routes()
        .merge(content::api::admin_routes(Kind::Blog))
        .merge(content::api::admin_routes(Kind::Project))
        .merge(content::api::admin_routes(Kind::CaseStudy))
        .merge(content::api::admin_routes(Kind::Service))
        .merge(content::visuals::routes())
        .merge(ai::api::routes())
        .merge(crate::domains::media::api::routes())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::api::require_auth,
        ));
    let public = content::api::public_routes(Kind::Blog)
        .merge(content::api::public_routes(Kind::Project))
        .merge(content::api::public_routes(Kind::CaseStudy))
        .merge(content::api::public_routes(Kind::Service));
    let ai_timeout = state
        .config
        .ai
        .as_ref()
        .map(|config| config.timeout_secs + 5)
        .unwrap_or(95);
    let timeout = middleware::from_fn(move |request: Request, next: Next| async move {
        let generation = request.uri().path() == "/api/v1/admin/ai/generate";
        match tokio::time::timeout(
            Duration::from_secs(if generation { ai_timeout } else { 30 }),
            next.run(request),
        )
        .await
        {
            Ok(response) => response,
            Err(_) if generation => AppError::AiTimeout.into_response(),
            Err(_) => (
                StatusCode::REQUEST_TIMEOUT,
                Json(json!({"error":{"code":"request_timeout","message":"Request timed out"}})),
            )
                .into_response(),
        }
    });
    Ok(Router::new()
        .route("/health/live",get(||async {Json(json!({"status":"ok"}))}))
        .route("/health/ready",get(ready))
        .route("/sitemap.xml",get(sitemap))
        .route("/robots.txt",get(robots))
        .route("/openapi.json",get(openapi))
        .route("/docs",get(||async {Html(include_str!("../../docs/swagger-ui.html"))}))
        .nest("/api/v1/auth",auth::api::routes(state.clone()))
        .nest("/api/v1/admin",admin)
        .nest("/api/v1",public)
        .merge(crate::domains::contact::routes())
        .fallback(||async {AppError::NotFound})
        .layer(DefaultBodyLimit::max(2*1024*1024))
        .layer(timeout)
        .layer(cors)
        .layer(middleware::from_fn(response_headers))
        .layer(TraceLayer::new_for_http().make_span_with(|request:&Request|tracing::info_span!("http",method=%request.method(),path=%request.uri().path())))
        .with_state(state))
}

async fn response_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    if path != "/sitemap.xml" && path != "/robots.txt" {
        response.headers_mut().insert(
            "x-robots-tag",
            HeaderValue::from_static("noindex, nofollow"),
        );
    }
    response
}
async fn ready(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM permissions)")
        .fetch_one(&state.pool)
        .await
    {
        Ok(true) => Json(json!({"status":"ready"})).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status":"not_ready"})),
        )
            .into_response(),
    }
}
async fn sitemap(State(state): State<AppState>) -> AppResult<Response> {
    Ok((
        [(CONTENT_TYPE, "application/xml; charset=utf-8")],
        content::seo::sitemap(&state.pool, &state.config).await?,
    )
        .into_response())
}
async fn robots(State(state): State<AppState>) -> Response {
    ([(CONTENT_TYPE,"text/plain; charset=utf-8")],format!("User-agent: *\nDisallow: /api/\nDisallow: /docs\nDisallow: /openapi.json\nDisallow: /health/\nSitemap: {}/sitemap.xml\n",state.config.api_url)).into_response()
}
async fn openapi() -> Json<Value> {
    let mut document =
        serde_json::from_str(include_str!("../../docs/openapi.json")).unwrap_or_else(|_| json!({}));
    ai::openapi::extend(&mut document);
    content::openapi::extend(&mut document);
    crate::domains::media::openapi::extend(&mut document);
    Json(document)
}
