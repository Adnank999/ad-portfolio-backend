use crate::domains::ai::{dto::*, validation};
use crate::{
    app::create_router,
    common::{config::Config, error::AppError, state::AppState},
    domains::{
        ai::{self, richtext, schema, service::authorize},
        auth::Principal,
    },
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Extension,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn request(kind: &str) -> GenerateRequest {
    serde_json::from_value(json!({
        "contentType":kind,"mode":"create","context":"I built a booking service in Rust using PostgreSQL and Redis.",
        "additionalInstructions":"","tone":"technical","audience":"software_developers","length":"medium",
        "language":"English","seoKeywords":[],"targetFields":["title","description"],"existingFields":{},
        "contentContext":{"type":kind}
    })).unwrap()
}

#[test]
fn typed_blog_and_project_requests_validate() {
    validation::request(&request("blog")).unwrap();
    validation::request(&request("project")).unwrap();
    validation::request(&request("case_study")).unwrap();
    validation::request(&request("service")).unwrap();
}

#[test]
fn service_generation_supports_structured_sections_without_visibility_or_media_changes() {
    let mut input = request("service");
    let records: serde_json::Value =
        serde_json::from_str(include_str!("../../../../examples/services.json")).unwrap();
    let page = records[0]["fields"]["pageContent"].clone();
    input.target_fields = vec![Field::PageContent, Field::NavTitle, Field::CanonicalUrl];
    input
        .existing_fields
        .insert("slug".into(), json!("web-development"));
    input
        .existing_fields
        .insert("fields.pageContent".into(), page.clone());
    validation::request(&input).unwrap();
    assert_eq!(
        validation::normalize(Field::PageContent, page.clone(), &input).unwrap(),
        page
    );
    assert_eq!(
        validation::derived(Field::CanonicalUrl, &input, "https://portfolio.test"),
        Some(json!("https://portfolio.test/services/web-development"))
    );
    input.target_fields = vec![Field::EditorContent];
    assert!(validation::request(&input).is_err());
    input.target_fields = vec![Field::PageContent];
    input
        .existing_fields
        .insert("fields.enabled".into(), json!(false));
    assert!(validation::request(&input).is_err());
}

#[test]
fn case_study_ai_supports_narrative_lessons_and_faqs_but_not_publication_or_project_fields() {
    let mut input = request("case_study");
    input.target_fields = vec![Field::Context, Field::Lessons, Field::Faqs];
    validation::request(&input).unwrap();
    let draft: ProviderDraft = serde_json::from_value(json!({"generatedFields":{"fields.context":"The service connects booking workflows.","fields.lessons":["Keep workflow states explicit."],"faqs":[{"question":"What is the study about?","answer":"A booking service in Rust."}]},"warnings":[]})).unwrap();
    let result = validation::output(&input, draft, "https://portfolio.test").unwrap();
    assert!(result.generated_fields.contains_key(&Field::Context));
    assert!(result.generated_fields.contains_key(&Field::Lessons));
    for field in [
        Field::Year,
        Field::Category,
        Field::CoverAlt,
        Field::Features,
    ] {
        input.target_fields = vec![field];
        assert!(validation::request(&input).is_err());
    }
    input.target_fields = vec![Field::CanonicalUrl];
    input
        .existing_fields
        .insert("slug".into(), json!("booking-study"));
    assert_eq!(
        validation::derived(Field::CanonicalUrl, &input, "https://portfolio.test"),
        Some(json!("https://portfolio.test/case-studies/booking-study"))
    );
}

#[test]
fn case_study_generation_requires_its_own_write_permission_and_an_admin_role() {
    let mut user = Principal {
        id: uuid::Uuid::nil(),
        session_id: uuid::Uuid::nil(),
        email: "test@example.invalid".into(),
        name: "Test".into(),
        headline: String::new(),
        roles: vec!["admin".into()],
        permissions: vec!["projects:write".into()],
    };
    assert!(authorize(&user, ContentType::CaseStudy).is_err());
    user.permissions.push("case-studies:write".into());
    assert!(authorize(&user, ContentType::CaseStudy).is_ok());
    user.roles = vec!["editor".into()];
    assert!(authorize(&user, ContentType::CaseStudy).is_err());
}

#[test]
fn malformed_types_fields_and_missing_context_are_rejected() {
    for (key, value) in [
        ("contentType", json!("video")),
        ("targetFields", json!(["status"])),
        (
            "contentContext",
            json!({"type":"project","notSupported":true}),
        ),
    ] {
        let mut value_request = serde_json::to_value(request("blog")).unwrap();
        value_request[key] = value;
        assert!(serde_json::from_value::<GenerateRequest>(value_request).is_err());
    }
    let mut input = request("blog");
    input.context.clear();
    assert!(validation::request(&input).is_err());
    let mut input = request("blog");
    input.target_fields = vec![Field::Features];
    assert!(validation::request(&input).is_err());
    let mut input = request("blog");
    input
        .existing_fields
        .insert("status".into(), json!("published"));
    assert!(validation::request(&input).is_err());
}

#[test]
fn safe_block_conversion_schema_and_unexpected_output_validation() {
    let document = richtext::document(json!([
        {"type":"heading","level":2,"content":[{"text":"Architecture","bold":false,"italic":false,"code":false}]},
        {"type":"paragraph","content":[{"text":"<script>not executable</script>","bold":true}]},
        {"type":"table","headers":["Tool","Purpose"],"rows":[["Redis","Cache"]]}
    ])).unwrap();
    assert_eq!(document["type"], "doc");
    let html = crate::domains::content::editor::render_document(&document).unwrap();
    assert!(!html.contains("<script>"));
    assert!(richtext::document(json!([{"type":"image","src":"javascript:alert(1)"}])).is_err());
    assert!(richtext::document(json!([{"type":"heading","level":1,"content":[]}])).is_err());
    let schema = schema::response(ContentType::Blog, &[Field::Title, Field::EditorContent]);
    assert!(schema
        .pointer("/properties/generatedFields/properties/editorContent")
        .is_some());
    assert!(schema
        .pointer("/properties/generatedFields/properties/fields.features")
        .is_none());
    let draft = ProviderDraft {
        generated_fields: [(Field::Technical, json!("unrequested"))].into(),
        warnings: vec![],
    };
    assert!(matches!(
        validation::output(&request("blog"), draft, "https://portfolio.test"),
        Err(AppError::AiInvalidOutput)
    ));
}

#[test]
fn normalizes_requested_values_and_preserves_facts_and_protected_assets() {
    let mut input = request("project");
    input.target_fields = vec![
        Field::Slug,
        Field::Technologies,
        Field::Results,
        Field::Year,
        Field::ImageAlt,
        Field::OgImage,
    ];
    input.content_context = ContentContext::Project(ProjectContext {
        technologies: vec!["Rust".into(), "Redis".into()],
        ..Default::default()
    });
    input
        .existing_fields
        .insert("fields.image".into(), json!("/images/known.webp"));
    input
        .existing_fields
        .insert("fields.year".into(), json!("2024"));
    let draft = ProviderDraft {
        generated_fields: [
            (Field::Slug, json!("Rust Booking Backend!")),
            (Field::Technologies, json!(["Rust", "Redis", "Rust"])),
            (
                Field::Results,
                json!("Delivered 90% less latency and 10000 users"),
            ),
            (Field::ImageAlt, json!("A glowing invented screen")),
        ]
        .into(),
        warnings: vec![],
    };
    let result = validation::output(&input, draft, "https://portfolio.test").unwrap();
    assert_eq!(
        result.generated_fields[&Field::Slug],
        json!("rust-booking-backend")
    );
    assert_eq!(
        result.generated_fields[&Field::Technologies],
        json!(["Rust", "Redis"])
    );
    assert_eq!(
        result.generated_fields[&Field::OgImage],
        json!("https://portfolio.test/images/known.webp")
    );
    for field in [Field::Results, Field::Year, Field::ImageAlt] {
        assert!(!result.generated_fields.contains_key(&field));
    }
    assert_eq!(input.existing_fields["fields.year"], json!("2024"));
    assert!(validation::normalize(Field::Technologies, json!(["Kubernetes"]), &input).is_err());
    assert!(validation::normalize(Field::Technologies, json!(["Postgre"]), &input).is_err());
}

#[test]
fn canonical_is_omitted_until_a_changed_slug_is_applied() {
    let mut input = request("blog");
    input.target_fields = vec![Field::Slug, Field::CanonicalUrl];
    input
        .existing_fields
        .insert("slug".into(), json!("old-slug"));
    let response = validation::output(
        &input,
        ProviderDraft {
            generated_fields: [(Field::Slug, json!("new-slug"))].into(),
            warnings: vec![],
        },
        "https://portfolio.test",
    )
    .unwrap();
    assert!(!response.generated_fields.contains_key(&Field::CanonicalUrl));
    assert!(response
        .skipped_fields
        .iter()
        .any(|field| field.field == Field::CanonicalUrl));
}

#[test]
fn fill_missing_and_faq_deduplication_are_non_destructive() {
    let mut input = request("blog");
    input.mode = Mode::FillMissing;
    input.target_fields = vec![Field::Title, Field::Faqs];
    input
        .existing_fields
        .insert("title".into(), json!("Manual title"));
    input.existing_fields.insert("faqs".into(), json!([]));
    let result = validation::output(&input,ProviderDraft { generated_fields:[(Field::Title,json!("AI title")),(Field::Faqs,json!([{"question":"Why Rust?","answer":"Typed backend code."},{"question":"Why Rust?","answer":"Duplicate."}]))].into(),warnings:vec![] },"https://portfolio.test").unwrap();
    assert!(!result.generated_fields.contains_key(&Field::Title));
    assert_eq!(
        result.generated_fields[&Field::Faqs]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let encoded = serde_json::to_value(result).unwrap();
    assert_eq!(encoded["contentType"], "blog");
    assert!(encoded["generationId"].is_string());
    assert!(encoded.get("status").is_none());
}

fn principal(role: &str) -> Principal {
    Principal {
        id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        email: "admin@test.invalid".into(),
        name: "Admin".into(),
        headline: String::new(),
        roles: vec![role.into()],
        permissions: vec!["blogs:write".into(), "projects:write".into()],
    }
}
async fn state() -> AppState {
    let config = Config {
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
        ai: None,
    };
    AppState::new(
        PgPoolOptions::new()
            .connect_lazy(&config.database_url)
            .unwrap(),
        config,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn authentication_authorization_and_unconfigured_provider_are_enforced() {
    let state = state().await;
    let app = create_router(state.clone()).unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/admin/ai/generate")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&request("blog")).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for role in ["viewer", "editor", "admin", "owner"] {
        let user = principal(role);
        assert_eq!(
            authorize(&user, ContentType::Blog).is_ok(),
            matches!(role, "admin" | "owner")
        );
        let app = ai::api::routes()
            .layer(Extension(user))
            .with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/generate")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&request("blog")).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if matches!(role, "admin" | "owner") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::FORBIDDEN
            }
        );
    }
    let mut administrator = principal("admin");
    administrator.permissions.clear();
    assert!(matches!(
        authorize(&administrator, ContentType::Blog),
        Err(AppError::Forbidden)
    ));
}
