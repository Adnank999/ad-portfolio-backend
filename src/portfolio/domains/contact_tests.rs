use super::*;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    response::IntoResponse,
};
use http_body_util::BodyExt;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

fn input() -> ContactInput {
    serde_json::from_value(json!({"name":"Alex Morgan","email":"alex@test.invalid","subject":"A new website","message":"Please help me build a booking website.","requestId":"12345678-1234-4234-8234-123456789abc"})).unwrap()
}

fn config() -> ContactConfig {
    ContactConfig {
        api_key: "test-only-resend-key".into(),
        from: "Portfolio <contact@portfolio.test>".into(),
        to: "owner@test.invalid".into(),
    }
}

async fn mock(
    status: StatusCode,
    body: &str,
    delay: Duration,
) -> (
    ContactService,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let body = body.to_owned();
    let app = Router::new().route(
        "/emails",
        post(
            move |headers: HeaderMap, Json(payload): Json<serde_json::Value>| {
                let count = count.clone();
                let body = body.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(headers["authorization"], "Bearer test-only-resend-key");
                    assert_eq!(
                        headers["idempotency-key"],
                        "contact/12345678-1234-4234-8234-123456789abc"
                    );
                    assert_eq!(payload["from"], "Portfolio <contact@portfolio.test>");
                    assert_eq!(payload["to"], json!(["owner@test.invalid"]));
                    assert_eq!(payload["reply_to"], "alex@test.invalid");
                    assert_eq!(payload["subject"], "Portfolio enquiry: A new website");
                    assert!(payload["text"]
                        .as_str()
                        .unwrap()
                        .contains("Name: Alex Morgan"));
                    assert!(payload.get("html").is_none());
                    tokio::time::sleep(delay).await;
                    (status, body).into_response()
                }
            },
        ),
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut service = ContactService::new(Some(config())).unwrap();
    service.endpoint = format!("http://{address}/emails");
    (service, calls, task)
}

#[test]
fn contact_rejects_injection_invalid_addresses_empty_oversized_and_spam_input() {
    for (field, value) in [
        ("name", "A"),
        ("name", "Alex\r\nBcc: attacker@test.invalid"),
        ("email", "bad@"),
        ("subject", "Hi\nBcc: attacker@test.invalid"),
        ("message", "short"),
        ("website", "https://spam.test"),
    ] {
        let mut value_json = serde_json::to_value(json!({"name":"Alex Morgan","email":"alex@test.invalid","subject":"A new website","message":"Please help me build a booking website.","requestId":Uuid::new_v4()})).unwrap();
        value_json[field] = json!(value);
        assert!(serde_json::from_value::<ContactInput>(value_json)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut invalid = input();
    invalid.message = "x".repeat(4001);
    assert!(invalid.validate().is_err());
    invalid = input();
    invalid.request_id = Uuid::nil();
    assert!(invalid.validate().is_err());
    let mut value = input();
    value.email = " ALEX@test.invalid ".into();
    value.validate().unwrap();
    assert_eq!(value.email, "alex@test.invalid");
    let mut invalid_config = config();
    invalid_config.from = "attacker\r\n@test.invalid".into();
    assert!(ContactService::new(Some(invalid_config)).is_err());
}

#[test]
fn contact_rate_limits_each_sender_and_total_volume_and_expires_windows() {
    let mut limits = Limits::default();
    let now = Instant::now();
    for _ in 0..3 {
        limits.check("one@test.invalid", now).unwrap();
    }
    assert!(matches!(
        limits.check("one@test.invalid", now),
        Err(AppError::ContactRateLimited)
    ));
    for n in 0..17 {
        limits
            .check(&format!("other{n}@test.invalid"), now)
            .unwrap();
    }
    assert!(matches!(
        limits.check("new@test.invalid", now),
        Err(AppError::ContactRateLimited)
    ));
    limits
        .check("one@test.invalid", now + Duration::from_secs(61))
        .unwrap();
    assert_eq!(limits.senders.len(), 1);
}

#[tokio::test]
async fn contact_sends_fixed_recipient_reply_to_plain_text_and_stable_retry_key() {
    let (service, calls, task) = mock(
        StatusCode::OK,
        r#"{"id":"aaaaaaaa-1234-4234-8234-123456789abc"}"#,
        Duration::ZERO,
    )
    .await;
    let first = service.send(input()).await.unwrap();
    let retry = service.send(input()).await.unwrap();
    assert_eq!(first.id, retry.id);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let _permits = service.slots.acquire_many(2).await.unwrap();
    assert!(matches!(
        service.send(input()).await,
        Err(AppError::ContactRateLimited)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn contact_unconfigured_provider_errors_malformed_success_and_timeout_never_confirm_send() {
    assert!(matches!(
        ContactService::new(None).unwrap().send(input()).await,
        Err(AppError::ContactUnavailable)
    ));
    for (status, body, delay) in [
        (
            StatusCode::UNAUTHORIZED,
            "private provider error",
            Duration::ZERO,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "private provider quota",
            Duration::ZERO,
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "private provider error",
            Duration::ZERO,
        ),
        (StatusCode::OK, r#"{"id":"invalid"}"#, Duration::ZERO),
        (
            StatusCode::OK,
            r#"{"id":"aaaaaaaa-1234-4234-8234-123456789abc"}"#,
            Duration::from_millis(200),
        ),
    ] {
        let (mut service, _, task) = mock(status, body, delay).await;
        service.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(80))
            .build()
            .unwrap();
        let error = service.send(input()).await.err().unwrap();
        assert!(matches!(error, AppError::ContactUnavailable));
        assert!(!error.to_string().contains("private"));
        task.abort();
    }
}

async fn app(contact: ContactService) -> Router {
    let config = crate::common::config::Config {
        database_url: "postgres://unused@127.0.0.1/unused".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        frontend_url: "https://portfolio.test".into(),
        api_url: "https://api.portfolio.test".into(),
        cors_origins: vec!["https://portfolio.test".into()],
        max_connections: 2,
        access_minutes: 30,
        refresh_days: 7,
        reset_email: None,
        contact: None,
        ai: None,
        r2: None,
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy(&config.database_url)
        .unwrap();
    let mut state = AppState::new(pool, config).await.unwrap();
    state.contact = Arc::new(contact);
    crate::app::create_router(state).unwrap()
}

async fn call(app: &Router, payload: serde_json::Value) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/contact")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn contact_public_route_sends_without_a_database_and_rejects_overposting() {
    let (service, calls, task) = mock(
        StatusCode::OK,
        r#"{"id":"aaaaaaaa-1234-4234-8234-123456789abc"}"#,
        Duration::ZERO,
    )
    .await;
    let app = app(service).await;
    let payload = json!({"name":"Alex Morgan","email":"alex@test.invalid","subject":"A new website","message":"Please help me build a booking website.","requestId":"12345678-1234-4234-8234-123456789abc"});
    assert_eq!(call(&app, payload.clone()).await.0, StatusCode::OK);
    let mut forged = payload.clone();
    forged["to"] = json!("attacker@test.invalid");
    assert_eq!(call(&app, forged).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    let mut huge = payload;
    huge["message"] = json!("x".repeat(25 * 1024));
    assert_eq!(call(&app, huge).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
}
