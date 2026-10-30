use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use portfolio_api::{
    app::create_router,
    common::{config::Config, security, state::AppState, validation},
    domains::{
        auth::service::bootstrap_owner,
        content::{
            dto::{ContentWrite, Kind},
            editor, service,
        },
    },
};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower::ServiceExt;

#[path = "portfolio_media.rs"]
mod media;

const PASSWORD: &str = "test-only-password-123!";
fn config() -> Config {
    Config {
        database_url: "postgres://unused:unused@127.0.0.1/unused".into(),
        host: "127.0.0.1".into(),
        port: 8080,
        frontend_url: "https://portfolio.test".into(),
        api_url: "https://api.portfolio.test".into(),
        cors_origins: vec!["https://portfolio.test".into()],
        max_connections: 5,
        access_minutes: 30,
        refresh_days: 7,
        reset_email: None,
        contact: None,
        ai: None,
        r2: None,
    }
}
struct Reply {
    status: StatusCode,
    body: Value,
    headers: HeaderMap,
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "https://portfolio.test");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(
                    body.map(|value| Body::from(value.to_string()))
                        .unwrap_or_else(Body::empty),
                )
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)))
    };
    Reply {
        status,
        body,
        headers,
    }
}
async fn setup(pool: PgPool) -> (AppState, Router, String) {
    let state = AppState::new(pool, config()).await.unwrap();
    bootstrap_owner(
        &state,
        "owner@test.invalid".into(),
        "Test Owner".into(),
        PASSWORD.into(),
    )
    .await
    .unwrap();
    let app = create_router(state.clone()).unwrap();
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"OWNER@test.invalid","password":PASSWORD})),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
    (
        state,
        app,
        login.body["accessToken"].as_str().unwrap().to_owned(),
    )
}
fn blog() -> Value {
    serde_json::from_str(include_str!("../examples/blog.json")).unwrap()
}
fn project() -> Value {
    serde_json::from_str(include_str!("../examples/project.json")).unwrap()
}

#[test]
fn validates_slugs_urls_and_passwords() {
    for email in [
        "a@.",
        "a..b@domain.test",
        "a@-domain.test",
        "<a>@domain.test",
        "a@domain..test",
    ] {
        assert!(validation::email(email).is_err());
    }
    for slug in ["a", "release-2026", "hotel-booking-platform"] {
        assert!(validation::slug(slug).is_ok());
    }
    for slug in ["", "UPPER", "-a", "a-", "a--b", "../escape", "a/b"] {
        assert!(validation::slug(slug).is_err());
    }
    for url in [
        "javascript:alert(1)",
        "data:image/svg+xml,x",
        "//evil.test/x",
        "/\\evil.test",
        "https://user:pass@site.test",
    ] {
        assert!(validation::url(url, "url", true, false).is_err());
    }
    assert!(validation::url("/images/cover.png", "image", true, false).is_ok());
    assert!(validation::url("https://site.test/image.png", "image", true, false).is_ok());
    assert!(validation::password("short").is_err());
    assert!(validation::password(PASSWORD).is_ok());
}

#[test]
fn frontend_examples_generate_safe_semantic_content() {
    for (kind, value) in [(Kind::Blog, blog()), (Kind::Project, project())] {
        let mut input: ContentWrite = serde_json::from_value(value).unwrap();
        let (html, plain) = service::prepare(kind, &mut input).unwrap();
        assert!(html.contains("<h2"));
        assert!(!plain.is_empty());
        assert!(!html.contains("<script"));
    }
    let mut malicious = blog();
    malicious["fields"]["intro"] = json!(["<script>alert('xss')</script>"]);
    let mut input = serde_json::from_value(malicious).unwrap();
    let (html, _) = service::prepare(Kind::Blog, &mut input).unwrap();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn tiptap_rendering_rejects_unsafe_and_unsupported_nodes() {
    let document = json!({"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"A heading"}]},{"type":"paragraph","content":[{"type":"text","text":"<script>danger</script>","marks":[{"type":"bold"}]}]}]});
    let rendered = editor::render_document(&document).unwrap();
    assert!(rendered.contains("<h2>A heading</h2>"));
    assert!(!rendered.contains("<script>"));
    for node in [
        json!({"type":"script","content":[]}),
        json!({"type":"heading","attrs":{"level":1}}),
        json!({"type":"paragraph","content":[{"type":"text","text":"click","marks":[{"type":"link","attrs":{"href":"javascript:alert(1)"}}]}]}),
        json!({"type":"image","attrs":{"src":"data:image/svg+xml,unsafe"}}),
    ] {
        assert!(editor::render_document(&json!({"type":"doc","content":[node]})).is_err());
    }
    let mut nested = json!({"type":"text","text":"deep"});
    for _ in 0..25 {
        nested = json!({"type":"paragraph","content":[nested]});
    }
    assert!(editor::render_document(&json!({"type":"doc","content":[nested]})).is_err());
}

#[test]
fn invalid_content_and_overposting_are_rejected() {
    let mut input = blog();
    input["fields"]["category"] = json!("unsupported");
    let mut parsed = serde_json::from_value(input).unwrap();
    assert!(service::prepare(Kind::Blog, &mut parsed).is_err());
    let mut input = blog();
    input["seo"]["canonicalUrl"] = json!("javascript:alert(1)");
    let mut parsed = serde_json::from_value(input).unwrap();
    assert!(service::prepare(Kind::Blog, &mut parsed).is_err());
    let mut input = project();
    input["fields"]["githubUrl"] = json!("#");
    let mut parsed = serde_json::from_value(input).unwrap();
    assert!(service::prepare(Kind::Project, &mut parsed).is_err());
    let mut input = blog();
    input["authorId"] = json!("attacker");
    assert!(serde_json::from_value::<ContentWrite>(input).is_err());
}

#[tokio::test]
async fn tokens_are_random_and_passwords_are_hashed() {
    let first = security::token();
    let second = security::token();
    assert_ne!(first, second);
    assert_eq!(first.len(), 64);
    assert_ne!(security::token_hash(&first).unwrap(), first);
    let pool = PgPoolOptions::new()
        .connect_lazy(&config().database_url)
        .unwrap();
    let state = AppState::new(pool, config()).await.unwrap();
    let hash = security::hash_password(PASSWORD.into()).await.unwrap();
    assert!(hash.starts_with("$argon2id$"));
    assert!(
        security::verify_password(&state, PASSWORD.into(), hash.clone())
            .await
            .unwrap()
    );
    assert!(!security::verify_password(&state, "wrong".into(), hash)
        .await
        .unwrap());
}

#[tokio::test]
async fn authentication_gate_rate_limit_and_headers_work_without_database() {
    let pool = PgPoolOptions::new()
        .connect_lazy(&config().database_url)
        .unwrap();
    let state = AppState::new(pool, config()).await.unwrap();
    let app = create_router(state).unwrap();
    for path in [
        "/api/v1/auth/me",
        "/api/v1/admin/blogs",
        "/api/v1/admin/projects",
        "/api/v1/admin/users",
        "/api/v1/admin/roles",
    ] {
        let reply = call(&app, "GET", path, None, None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.headers["cache-control"], "no-store");
        assert_eq!(reply.headers["x-robots-tag"], "noindex, nofollow");
    }
    assert_eq!(
        call(&app, "GET", "/health/live", None, None).await.status,
        StatusCode::OK
    );
    let docs = call(&app, "GET", "/docs", None, None).await;
    assert_eq!(docs.status, StatusCode::OK);
    assert!(docs.headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    assert!(docs.body.as_str().unwrap().contains("/openapi.json"));
    for _ in 0..10 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/auth/login",
                None,
                Some(json!({"email":"invalid","password":"invalid"}))
            )
            .await
            .status,
            StatusCode::UNAUTHORIZED
        );
    }
    let limited = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"invalid","password":"invalid"})),
    )
    .await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers["retry-after"], "60");
}

#[test]
fn openapi_is_valid_and_documents_both_content_domains() {
    let schema: Value = serde_json::from_str(include_str!("../docs/openapi.json")).unwrap();
    assert!(schema["paths"]["/api/v1/admin/blogs/{id}"]["put"].is_object());
    assert!(schema["paths"]["/api/v1/projects/{slug}"]["get"].is_object());
    assert!(schema["paths"]["/docs"]["get"].is_object());
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn case_studies_import_edit_media_seo_faqs_and_publication(pool: PgPool) {
    let (state, app, token) = setup(pool).await;
    assert_eq!(
        portfolio_api::domains::content::import::import_case_studies(&state.pool)
            .await
            .unwrap(),
        13
    );
    let listed = call(
        &app,
        "GET",
        "/api/v1/admin/case-studies",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["pagination"]["total"], 13);
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/case-studies", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let item = &listed.body["data"][0];
    let id = item["id"].as_str().unwrap();
    let original_slug = item["slug"].as_str().unwrap().to_owned();
    let detail = call(
        &app,
        "GET",
        &format!("/api/v1/admin/case-studies/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert!(!detail.body["fields"]["context"]
        .as_str()
        .unwrap()
        .is_empty());
    assert!(!detail.body["faqs"].as_array().unwrap().is_empty());
    let mut input = json!({
        "slug":"edited-case-study", "title":"Updated case study", "description":detail.body["description"],
        "status":"published", "version":detail.body["version"], "fields":detail.body["fields"],
        "seo":{"metaTitle":"Edited SEO title","metaDescription":"Edited search description","structuredData":{"@context":"https://schema.org","@type":"SoftwareApplication","name":"Verified software"}},
        "faqs":[{"question":"How was this reviewed?","answer":"With documented workflow tests."}]
    });
    input["fields"]["image"] = json!("/uploads/case-cover.webp");
    input["fields"]["imageAlt"] = json!("Uploaded application cover");
    input["fields"]["gallery"] = json!([{"url":"/uploads/detail.webp","alt":"Application detail","caption":"A verified screenshot"}]);
    let updated = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/case-studies/{id}"),
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.body["version"], 2);
    assert_eq!(
        updated.body["fields"]["gallery"],
        input["fields"]["gallery"]
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/case-studies/{id}"),
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let public = call(
        &app,
        "GET",
        "/api/v1/case-studies/edited-case-study",
        None,
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.body["image"], "/uploads/case-cover.webp");
    assert_eq!(public.body["seo"]["metaTitle"], "Edited SEO title");
    assert_eq!(
        public.body["seo"]["canonicalUrl"],
        "https://portfolio.test/case-studies/edited-case-study"
    );
    let graph = public.body["structuredData"]["@graph"].as_array().unwrap();
    assert!(graph
        .iter()
        .any(|node| node["@type"] == "SoftwareApplication"));
    assert!(graph.iter().any(|node| node["@type"] == "FAQPage"
        && node["mainEntity"][0]["name"] == "How was this reviewed?"));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/case-studies/{original_slug}"),
            None,
            None
        )
        .await
        .status,
        StatusCode::PERMANENT_REDIRECT
    );
    assert_eq!(
        portfolio_api::domains::content::import::import_case_studies(&state.pool)
            .await
            .unwrap(),
        0
    );
    let preserved = call(
        &app,
        "GET",
        &format!("/api/v1/admin/case-studies/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(preserved.body["title"], "Updated case study");
    assert_eq!(
        preserved.body["fields"]["image"],
        "/uploads/case-cover.webp"
    );
    let sitemap = call(&app, "GET", "/sitemap.xml", None, None).await;
    assert!(sitemap
        .body
        .as_str()
        .unwrap()
        .contains("/case-studies/edited-case-study"));
    input["slug"] = json!("draft-case-study");
    input["status"] = json!("draft");
    input.as_object_mut().unwrap().remove("version");
    let draft = call(
        &app,
        "POST",
        "/api/v1/admin/case-studies",
        Some(&token),
        Some(input),
    )
    .await;
    assert_eq!(draft.status, StatusCode::CREATED);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/case-studies/draft-case-study",
            None,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let archived_input = json!({"slug":"edited-case-study","title":"Updated case study","description":updated.body["description"],"status":"archived","version":2,"fields":updated.body["fields"],"faqs":updated.body["faqs"],"seo":updated.body["seo"]});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/case-studies/{id}"),
            Some(&token),
            Some(archived_input)
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/case-studies/edited-case-study",
            None,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn content_publication_versions_redirects_and_sitemap(pool: PgPool) {
    let (_state, app, token) = setup(pool).await;
    let mut input = blog();
    let created = call(
        &app,
        "POST",
        "/api/v1/admin/blogs",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.body["id"].as_str().unwrap();
    let slug = input["slug"].as_str().unwrap().to_owned();
    assert_eq!(
        call(&app, "GET", &format!("/api/v1/blogs/{slug}"), None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/blogs", None, None).await.body["pagination"]["total"],
        0
    );
    input["status"] = json!("published");
    input["version"] = json!(1);
    let published = call(
        &app,
        "PUT",
        &format!("/api/v1/admin/blogs/{id}"),
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(published.body["version"], 2);
    let public = call(&app, "GET", &format!("/api/v1/blogs/{slug}"), None, None).await;
    assert_eq!(public.status, StatusCode::OK, "{}", public.body);
    assert_eq!(public.body["sections"], input["fields"]["sections"]);
    assert_eq!(public.body["faqs"], input["faqs"]);
    assert!(public.body["seo"]["canonicalUrl"]
        .as_str()
        .unwrap()
        .ends_with(&slug));
    assert_eq!(
        public.body["structuredData"]["@graph"][2]["@type"],
        "FAQPage"
    );
    assert!(public.body.get("email").is_none());
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{id}"),
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    input["slug"] = json!("a-better-first-release");
    input["version"] = json!(2);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{id}"),
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::OK
    );
    let moved = call(&app, "GET", &format!("/api/v1/blogs/{slug}"), None, None).await;
    assert_eq!(moved.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        moved.headers["location"],
        "/api/v1/blogs/a-better-first-release"
    );
    let sitemap = call(&app, "GET", "/sitemap.xml", None, None).await;
    assert!(sitemap
        .body
        .as_str()
        .unwrap()
        .contains("https://portfolio.test/blog/a-better-first-release"));
    input["seo"]["noIndex"] = json!(true);
    input["version"] = json!(3);
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{id}"),
            Some(&token),
            Some(input)
        )
        .await
        .status,
        StatusCode::OK
    );
    assert!(!call(&app, "GET", "/sitemap.xml", None, None)
        .await
        .body
        .as_str()
        .unwrap()
        .contains("a-better-first-release"));
    let mut scheduled = blog();
    scheduled["slug"] = json!("a-future-release");
    scheduled["status"] = json!("published");
    scheduled["publishedAt"] = json!((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(&token),
            Some(scheduled)
        )
        .await
        .status,
        StatusCode::CREATED
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/blogs/a-future-release", None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/blogs?category=insights&tag=Project%20planning&q=release&perPage=1",
            None,
            None
        )
        .await
        .body["pagination"]["total"],
        1
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/blogs?perPage=101", None, None)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/blogs/{id}?version=3"),
            Some(&token),
            None
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/blogs/{id}?version=4"),
            Some(&token),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/blogs/a-better-first-release",
            None,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn project_contract_and_editor_content(pool: PgPool) {
    let (_state, app, token) = setup(pool).await;
    let mut input = project();
    input["fields"]["thumbnailUrl"] = json!("/images/project-thumbnail.webp");
    input["fields"]["gallery"] =
        json!([{"url":"/images/project-featured.webp","alt":"Chosen featured image"}]);
    input["fields"]["featuredImageUrl"] = json!("/images/project-featured.webp");
    input["status"] = json!("published");
    let created = call(
        &app,
        "POST",
        "/api/v1/admin/projects",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let public = call(
        &app,
        "GET",
        "/api/v1/projects/hotel-booking-platform",
        None,
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.body["features"], input["fields"]["features"]);
    assert_eq!(public.body["technologies"], input["fields"]["technologies"]);
    assert_eq!(
        public.body["thumbnailUrl"],
        "/images/project-thumbnail.webp"
    );
    assert_eq!(
        public.body["featuredImageUrl"],
        "/images/project-featured.webp"
    );
    assert_eq!(
        public.body["seo"]["ogImage"],
        "https://portfolio.test/images/project-featured.webp"
    );
    assert_eq!(public.body["seo"]["ogImageAlt"], "Chosen featured image");
    assert_eq!(
        public.body["structuredData"]["@graph"][0]["@type"],
        "CreativeWork"
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/projects?featured=true", None, None)
            .await
            .body["pagination"]["total"],
        1
    );
    input["version"] = json!(1);
    input["editorContent"] = json!({"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"A new overview"}]},{"type":"paragraph","content":[{"type":"text","text":"<script>never execute</script>"}]}]});
    let updated = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/projects/{}",
            created.body["id"].as_str().unwrap()
        ),
        Some(&token),
        Some(input),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert!(!updated.body["renderedHtml"]
        .as_str()
        .unwrap()
        .contains("<script>"));
    assert!(updated.body["renderedHtml"]
        .as_str()
        .unwrap()
        .contains("A new overview"));
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn public_projects_exclude_drafts_archived_scheduled_and_deleted_records(pool: PgPool) {
    let (_, app, token) = setup(pool).await;
    for (slug, status) in [
        ("public-project", "published"),
        ("draft-project", "draft"),
        ("archived-project", "archived"),
        ("scheduled-project", "published"),
        ("deleted-project", "published"),
    ] {
        let mut input = project();
        input["slug"] = json!(slug);
        input["status"] = json!(status);
        if slug == "scheduled-project" {
            input["publishedAt"] =
                json!((chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339());
        }
        let created = call(
            &app,
            "POST",
            "/api/v1/admin/projects",
            Some(&token),
            Some(input),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
        if slug == "deleted-project" {
            let id = created.body["id"].as_str().unwrap();
            assert_eq!(
                call(
                    &app,
                    "DELETE",
                    &format!("/api/v1/admin/projects/{id}?version=1"),
                    Some(&token),
                    None
                )
                .await
                .status,
                StatusCode::NO_CONTENT
            );
        }
    }
    let archive = call(&app, "GET", "/api/v1/projects?perPage=100", None, None).await;
    assert_eq!(archive.status, StatusCode::OK);
    assert_eq!(archive.body["pagination"]["total"], 1);
    assert_eq!(archive.body["data"][0]["slug"], "public-project");
    for slug in [
        "draft-project",
        "archived-project",
        "scheduled-project",
        "deleted-project",
        "not-in-database",
    ] {
        assert_eq!(
            call(&app, "GET", &format!("/api/v1/projects/{slug}"), None, None)
                .await
                .status,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        call(&app, "GET", "/api/v1/projects/public-project", None, None)
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn rbac_live_updates_owner_protection_and_editor_limits(pool: PgPool) {
    let (state, app, owner) = setup(pool).await;
    let user = call(
        &app,
        "POST",
        "/api/v1/admin/users",
        Some(&owner),
        Some(json!({"email":"editor@test.invalid","name":"Editor","password":PASSWORD})),
    )
    .await;
    assert_eq!(user.status, StatusCode::CREATED);
    let id = user.body["id"].as_str().unwrap();
    assert!(user.body.get("passwordHash").is_none());
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"editor@test.invalid","password":PASSWORD})),
    )
    .await;
    let editor = login.body["accessToken"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(editor),
            Some(blog())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/users/{id}/roles"),
            Some(&owner),
            Some(json!({"roleIds":["00000000-0000-0000-0000-000000000003"]}))
        )
        .await
        .status,
        StatusCode::OK
    );
    let created = call(
        &app,
        "POST",
        "/api/v1/admin/blogs",
        Some(editor),
        Some(blog()),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let mut publish = blog();
    publish["version"] = json!(1);
    publish["status"] = json!("published");
    let content_id = created.body["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{content_id}"),
            Some(editor),
            Some(publish.clone())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{content_id}"),
            Some(&owner),
            Some(publish.clone())
        )
        .await
        .status,
        StatusCode::OK
    );
    publish["version"] = json!(2);
    publish["status"] = json!("draft");
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/blogs/{content_id}"),
            Some(editor),
            Some(publish)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let owner_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='owner@test.invalid'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/users/{owner_id}"),
            Some(&owner),
            Some(json!({"name":"Owner","headline":"Owner","active":false}))
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/users/{id}/roles"),
            Some(&owner),
            Some(json!({"roleIds":["00000000-0000-0000-0000-000000000001"]}))
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let role=call(&app,"POST","/api/v1/admin/roles",Some(&owner),Some(json!({"name":"blog-reviewer","description":"Read drafts","permissions":["content:read"]}))).await;
    assert_eq!(role.status, StatusCode::CREATED);
    let role_id = role.body["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/users/{id}/roles"),
            Some(&owner),
            Some(json!({"roleIds":[role_id]}))
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(editor),
            Some(blog())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/roles/{role_id}"),
            Some(&owner),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/users/{id}"),
            Some(&owner),
            Some(json!({"name":"Editor","headline":"Contributor","active":false}))
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", Some(editor), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn sessions_rotate_detect_replay_and_revoke_on_password_change(pool: PgPool) {
    let (state, app, owner) = setup(pool).await;
    let session = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"owner@test.invalid","password":PASSWORD})),
    )
    .await;
    let refresh = session.body["refreshToken"].as_str().unwrap();
    let old_access = session.body["accessToken"].as_str().unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT token_hash FROM auth_sessions WHERE token_hash=$1")
            .bind(security::token_hash(old_access).unwrap())
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_ne!(stored, old_access);
    let rotated = call(
        &app,
        "POST",
        "/api/v1/auth/refresh",
        None,
        Some(json!({"refreshToken":refresh})),
    )
    .await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", Some(old_access), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({"refreshToken":refresh}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/auth/me",
            rotated.body["accessToken"].as_str(),
            None
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/change-password",
            Some(&owner),
            Some(json!({"currentPassword":PASSWORD,"newPassword":"another-test-password-123!"}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", Some(&owner), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let signed_in = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"owner@test.invalid","password":"another-test-password-123!"})),
    )
    .await;
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/logout",
            signed_in.body["accessToken"].as_str(),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({"refreshToken":signed_in.body["refreshToken"]}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn concurrent_refresh_and_absolute_session_expiry(pool: PgPool) {
    let (state, app, owner) = setup(pool).await;
    let session = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"owner@test.invalid","password":PASSWORD})),
    )
    .await;
    let refresh = session.body["refreshToken"].as_str().unwrap();
    let (first, second) = tokio::join!(
        call(
            &app,
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({"refreshToken":refresh}))
        ),
        call(
            &app,
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({"refreshToken":refresh}))
        )
    );
    assert!(
        (first.status == StatusCode::OK && second.status == StatusCode::UNAUTHORIZED)
            || (second.status == StatusCode::OK && first.status == StatusCode::UNAUTHORIZED)
    );
    let successful = if first.status == StatusCode::OK {
        first
    } else {
        second
    };
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/auth/me",
            successful.body["accessToken"].as_str(),
            None
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query(
        "UPDATE auth_sessions SET refresh_expires_at=now()-interval '1 second' WHERE token_hash=$1",
    )
    .bind(security::token_hash(&owner).unwrap())
    .execute(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", Some(&owner), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn registration_has_no_permissions_and_reset_revokes_sessions(pool: PgPool) {
    let (state, app, _) = setup(pool).await;
    let registered = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({"email":"reader@test.invalid","name":"New Reader","password":"reader-password-123!"})),
    )
    .await;
    assert_eq!(registered.status, StatusCode::CREATED);
    let signed_in = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"reader@test.invalid","password":"reader-password-123!"})),
    )
    .await;
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_eq!(signed_in.body["user"]["roles"], json!([]));
    assert_eq!(signed_in.body["user"]["permissions"], json!([]));
    let access = signed_in.body["accessToken"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/blogs", Some(access), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let reset_token = security::token();
    let user_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='reader@test.invalid'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO password_reset_tokens(token_hash,user_id,expires_at) VALUES($1,$2,now()+interval '15 minutes')")
        .bind(security::token_hash(&reset_token).unwrap())
        .bind(user_id)
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/reset-password",
            None,
            Some(json!({"token":reset_token,"password":"new-reader-password-123!"}))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/auth/me", Some(access), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/auth/reset-password",
            None,
            Some(json!({"token":reset_token,"password":"third-reader-password-123!"}))
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; see PORTFOLIO.md"]
async fn services_cms_preserves_defaults_versions_visibility_media_seo_and_slug_history(
    pool: PgPool,
) {
    let (state, app, token) = setup(pool).await;
    use portfolio_api::domains::content::import::import_services;
    assert_eq!(import_services(&state.pool).await.unwrap(), 13);
    assert_eq!(import_services(&state.pool).await.unwrap(), 0);
    let archive = call(&app, "GET", "/api/v1/services?perPage=100", None, None).await;
    assert_eq!(archive.status, StatusCode::OK);
    assert_eq!(archive.body["pagination"]["total"], 13);
    assert!(archive.body["data"][0].get("pageContent").is_none());
    let listing = call(
        &app,
        "GET",
        "/api/v1/admin/services?perPage=100",
        Some(&token),
        None,
    )
    .await;
    let item = listing.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["slug"] == "web-development")
        .unwrap();
    assert_eq!(item["enabled"], true);
    let id = item["id"].as_str().unwrap();
    let path = format!("/api/v1/admin/services/{id}");
    let detail = call(&app, "GET", &path, Some(&token), None).await;
    let seeds: Vec<Value> =
        serde_json::from_str(include_str!("../examples/services.json")).unwrap();
    let mut input = seeds
        .into_iter()
        .find(|item| item["slug"] == "web-development")
        .unwrap();
    assert_eq!(detail.body["fields"], input["fields"]);
    assert_eq!(detail.body["faqs"], input["faqs"]);
    input["version"] = detail.body["version"].clone();
    input["fields"]["navEnabled"] = json!(false);
    input["fields"]["navTitle"] = json!("Website engineering");
    input["fields"]["navDescription"] = json!("Editable database navigation copy");
    input["fields"]["media"] = json!([{"slot":"hero","kind":"video","url":"/uploads/demo.mp4","alt":"Website walkthrough","poster":"/uploads/poster.webp","caption":"Product demonstration"}]);
    input["seo"]["metaTitle"] = json!("Custom service metadata");
    input["seo"]["structuredData"] = json!({"@context":"https://schema.org","@type":"WebPage","name":"Verified service details"});
    let saved = call(&app, "PUT", &path, Some(&token), Some(input.clone())).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(input.clone()))
            .await
            .status,
        StatusCode::CONFLICT
    );
    let public = call(&app, "GET", "/api/v1/services/web-development", None, None).await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.body["navEnabled"], false);
    assert_eq!(public.body["navTitle"], "Website engineering");
    assert_eq!(public.body["media"][0]["kind"], "video");
    assert_eq!(public.body["seo"]["metaTitle"], "Custom service metadata");
    assert_eq!(
        public.body["seo"]["ogImage"],
        "https://portfolio.test/uploads/poster.webp"
    );
    assert_eq!(
        public.body["seo"]["canonicalUrl"],
        "https://portfolio.test/services/web-development"
    );
    assert!(public.body["structuredData"]["@graph"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["@type"] == "Service"));
    assert!(public.body["structuredData"]["@graph"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["@type"] == "FAQPage"));
    assert_eq!(import_services(&state.pool).await.unwrap(), 0);
    assert_eq!(
        call(&app, "GET", &path, Some(&token), None).await.body["fields"]["navTitle"],
        "Website engineering"
    );
    input["version"] = saved.body["version"].clone();
    input["fields"]["enabled"] = json!(false);
    let disabled = call(&app, "PUT", &path, Some(&token), Some(input.clone())).await;
    assert_eq!(disabled.status, StatusCode::OK);
    assert_eq!(
        call(&app, "GET", "/api/v1/services/web-development", None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/services?perPage=100", None, None)
            .await
            .body["pagination"]["total"],
        12
    );
    let sitemap = call(&app, "GET", "/sitemap.xml", None, None).await;
    assert!(!sitemap
        .body
        .as_str()
        .unwrap()
        .contains("/services/web-development"));
    input["version"] = disabled.body["version"].clone();
    input["fields"]["enabled"] = json!(true);
    input["slug"] = json!("website-engineering");
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(input.clone()))
            .await
            .status,
        StatusCode::OK
    );
    let alias = call(&app, "GET", "/api/v1/services/web-development", None, None).await;
    assert_eq!(alias.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        alias.headers["location"],
        "/api/v1/services/website-engineering"
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/services/website-engineering",
            None,
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/services", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}
