use super::{blog, call, config, project, PASSWORD};
use async_trait::async_trait;
use axum::{http::StatusCode, Router};
use image::{codecs::webp::WebPEncoder, ExtendedColorType};
use portfolio_api::{
    app::create_router,
    common::{
        error::{AppError, AppResult},
        state::AppState,
    },
    domains::{
        auth::service::bootstrap_owner,
        media::{
            config::R2Config,
            storage::{ObjectStorage, StoredObject},
        },
    },
};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use uuid::Uuid;

#[derive(Default)]
struct MemoryStorage {
    objects: Mutex<HashMap<(bool, String), StoredObject>>,
    copies: AtomicUsize,
    fail_copy: AtomicBool,
    fail_delete: AtomicBool,
}

#[async_trait]
impl ObjectStorage for MemoryStorage {
    async fn presign(&self, key: &str) -> AppResult<(String, BTreeMap<String, String>)> {
        Ok((
            format!(
                "https://{}.r2.cloudflarestorage.com/private/{key}?signature=opaque",
                "a".repeat(32)
            ),
            BTreeMap::from([
                ("content-type".into(), "image/webp".into()),
                ("if-none-match".into(), "*".into()),
            ]),
        ))
    }
    async fn inspect(&self, public: bool, key: &str) -> AppResult<Option<StoredObject>> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(&(public, key.into()))
            .cloned())
    }
    async fn publish(
        &self,
        staging: &str,
        public: &str,
        etag: &str,
        upload_id: &str,
    ) -> AppResult<()> {
        if self.fail_copy.load(Ordering::SeqCst) {
            return Err(AppError::StorageUnavailable);
        }
        let mut objects = self.objects.lock().unwrap();
        let mut object = objects
            .get(&(false, staging.into()))
            .cloned()
            .ok_or(AppError::StorageUnavailable)?;
        assert_eq!(object.etag, etag);
        object.upload_id = Some(upload_id.into());
        objects.insert((true, public.into()), object);
        self.copies.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn delete(&self, public: bool, key: &str) -> AppResult<()> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(AppError::StorageUnavailable);
        }
        self.objects.lock().unwrap().remove(&(public, key.into()));
        Ok(())
    }
}

fn webp() -> Vec<u8> {
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes)
        .encode(&[128; 16 * 9 * 4], 16, 9, ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

async fn setup(pool: PgPool) -> (AppState, Router, String, Arc<MemoryStorage>) {
    let mut state = AppState::new(pool, config()).await.unwrap();
    let storage = Arc::new(MemoryStorage::default());
    let media = Arc::get_mut(&mut state.media).unwrap();
    media.config = Some(R2Config {
        account_id: "a".repeat(32),
        access_key_id: "test".into(),
        secret_access_key: "test".into(),
        staging_bucket: "test-private".into(),
        public_bucket: "test-public".into(),
        public_base_url: "https://media.test".into(),
        ttl_seconds: 300,
    });
    media.storage = Some(storage.clone());
    state.config.r2 = media.config.clone();
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
        Some(json!({"email":"owner@test.invalid","password":PASSWORD})),
    )
    .await;
    let token = login.body["accessToken"].as_str().unwrap().to_owned();
    (state, app, token, storage)
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn shader_activation_reports_missing_migration_and_recovers_after_migrating(pool: PgPool) {
    let (state, app, token, _storage) = setup(pool).await;
    let initial = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(initial.status, StatusCode::OK);
    let entry = &initial.body["shaders"][0];
    sqlx::raw_sql(
        "DROP TABLE shader_activations; DELETE FROM _sqlx_migrations WHERE version=202610270001;",
    )
    .execute(&state.pool)
    .await
    .unwrap();
    let missing = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(missing.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(missing.body["error"]["code"], "shader_schema_unavailable");
    assert!(missing.body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Apply pending backend database migrations"));
    assert!(!missing.body.to_string().contains("SELECT"));
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/shaders", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let path = format!(
        "/api/v1/admin/shaders/{}/activation",
        entry["id"].as_str().unwrap()
    );
    let input = json!({"version":entry["version"],"contract":entry["contract"],"active":false});
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(input.clone()))
            .await
            .status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let mut transaction = state.pool.begin().await.unwrap();
    assert!(matches!(
        portfolio_api::domains::content::visuals::validate_assignments(
            &mut transaction,
            &json!({"visuals":{"hero":{"type":"shader","shader":entry["id"],"version":entry["version"]}}}),
            None,
        ).await,
        Err(AppError::ShaderSchemaUnavailable)
    ));
    transaction.rollback().await.unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/services", None, None)
            .await
            .status,
        StatusCode::OK
    );
    portfolio_api::MIGRATOR.run(&state.pool).await.unwrap();
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(input))
            .await
            .status,
        StatusCode::OK
    );
    let recovered = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(recovered.status, StatusCode::OK);
    assert_eq!(recovered.body["shaders"][0]["active"], false);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn service_shaders_save_without_any_poster_upload_or_media_reference(pool: PgPool) {
    let (state, app, token, _storage) = setup(pool).await;
    let manifest = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(manifest.status, StatusCode::OK);
    let entry = &manifest.body["shaders"][0];
    let activated = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/shaders/{}/activation",
            entry["id"].as_str().unwrap()
        ),
        Some(&token),
        Some(json!({"version":entry["version"],"contract":entry["contract"],"active":true})),
    )
    .await;
    assert_eq!(activated.status, StatusCode::OK);
    let seeds: Vec<Value> =
        serde_json::from_str(include_str!("../examples/services.json")).unwrap();
    let mut input = seeds[0].clone();
    input["seo"] = json!({});
    input["assetIds"] = json!([]);
    input["fields"]["media"] = json!([]);
    input["fields"]["visuals"] = json!({});
    for slot in [
        "hero",
        "overview",
        "spotlight",
        "article-1",
        "article-2",
        "article-3",
    ] {
        input["fields"]["visuals"][slot] = json!({"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"alt":"Crimson background effect"});
    }
    let saved = call(
        &app,
        "POST",
        "/api/v1/admin/services",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    assert_eq!(saved.body["assetIds"], json!([]));
    assert_eq!(saved.body["fields"]["visuals"], input["fields"]["visuals"]);
    let public = call(
        &app,
        "GET",
        &format!("/api/v1/services/{}", input["slug"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.body["visuals"], input["fields"]["visuals"]);
    assert!(public.body["seo"]["ogImage"]
        .as_str()
        .is_some_and(|image| !image.is_empty()));
    input["version"] = saved.body["version"].clone();
    input["fields"]["visuals"]["hero"]["poster"] = json!("");
    let path = format!(
        "/api/v1/admin/services/{}",
        saved.body["id"].as_str().unwrap()
    );
    let edited = call(&app, "PUT", &path, Some(&token), Some(input.clone())).await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    input["version"] = edited.body["version"].clone();
    input["fields"]["visuals"]["hero"]["poster"] = json!("https://external.test/unverified.webp");
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(input))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let references: i64 = sqlx::query_scalar("SELECT count(*) FROM content_media_assets")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(references, 0);
}

async fn sign(app: &Router, token: &str, purpose: &str) -> Value {
    let reply = call(app, "POST", "/api/v1/admin/uploads/presign", Some(token), Some(json!({"purpose":purpose,"originalName":"../../source.png","contentType":"image/webp","sizeBytes":webp().len(),"width":16,"height":9}))).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body
}

fn upload(storage: &MemoryStorage, signed: &Value, bytes: Vec<u8>, content_type: &str) {
    let url = url::Url::parse(signed["uploadUrl"].as_str().unwrap()).unwrap();
    let key = url.path().strip_prefix("/private/").unwrap().to_owned();
    storage.objects.lock().unwrap().insert(
        (false, key),
        StoredObject {
            bytes,
            content_type: content_type.into(),
            etag: "opaque-etag".into(),
            upload_id: None,
        },
    );
}

async fn complete(app: &Router, token: &str, signed: &Value) -> super::Reply {
    call(
        app,
        "POST",
        &format!(
            "/api/v1/admin/uploads/{}/complete",
            signed["uploadId"].as_str().unwrap()
        ),
        Some(token),
        Some(json!({})),
    )
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn service_shader_visuals_persist_validated_settings_and_protect_their_ready_posters(
    pool: PgPool,
) {
    let (state, app, token, storage) = setup(pool).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/shaders", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let manifest = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(manifest.status, StatusCode::OK);
    let entries = manifest.body["shaders"].as_array().unwrap();
    let configured = portfolio_api::domains::content::visuals::catalog().unwrap();
    assert_eq!(
        entries.len(),
        configured.manifest["shaders"].as_array().unwrap().len()
    );
    assert!(!entries.is_empty());
    for entry in entries {
        if entry["active"] == false {
            let path = format!(
                "/api/v1/admin/shaders/{}/activation",
                entry["id"].as_str().unwrap()
            );
            let activated = call(
                &app,
                "PUT",
                &path,
                Some(&token),
                Some(
                    json!({"version":entry["version"],"contract":entry["contract"],"active":true}),
                ),
            )
            .await;
            assert_eq!(activated.status, StatusCode::OK, "{}", activated.body);
        }
    }
    let signed = sign(&app, &token, "service_image").await;
    upload(&storage, &signed, webp(), "image/webp");
    let poster = complete(&app, &token, &signed).await;
    assert_eq!(poster.status, StatusCode::OK);
    let seeds: Vec<Value> =
        serde_json::from_str(include_str!("../examples/services.json")).unwrap();
    let mut input = seeds[0].clone();
    input["seo"] = json!({});
    input["assetIds"] = json!([poster.body["assetId"]]);
    input["fields"]["visuals"] = json!({});
    for (index, slot) in [
        "hero",
        "overview",
        "spotlight",
        "article-1",
        "article-2",
        "article-3",
    ]
    .iter()
    .enumerate()
    {
        let entry = &entries[index % entries.len()];
        input["fields"]["visuals"][*slot] = json!({"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"poster":poster.body["publicUrl"],"alt":"Approved poster"});
    }
    let saved = call(
        &app,
        "POST",
        "/api/v1/admin/services",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    assert_eq!(saved.body["fields"]["visuals"], input["fields"]["visuals"]);
    let read = call(
        &app,
        "GET",
        &format!("/api/v1/services/{}", input["slug"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body["visuals"], input["fields"]["visuals"]);
    assert_eq!(read.body["seo"]["ogImage"], poster.body["publicUrl"]);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content_media_assets WHERE asset_id=$1")
            .bind(Uuid::parse_str(poster.body["assetId"].as_str().unwrap()).unwrap())
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    for (key, value) in [
        ("version", json!(999)),
        ("shader", json!("draft-effect")),
        ("poster", json!("https://external.test/unverified.webp")),
    ] {
        let mut invalid = input.clone();
        invalid["fields"]["visuals"]["hero"][key] = value;
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/admin/services",
                Some(&token),
                Some(invalid)
            )
            .await
            .status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut invalid = input.clone();
    invalid["fields"]["visuals"]["hero"]["props"]["onClick"] = json!("alert(1)");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/services",
            Some(&token),
            Some(invalid)
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut edited = input.clone();
    edited["version"] = saved.body["version"].clone();
    edited["fields"]["visuals"]["overview"] =
        json!({"type":"image","src":poster.body["publicUrl"],"alt":"Approved still image"});
    let updated = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/services/{}",
            saved.body["id"].as_str().unwrap()
        ),
        Some(&token),
        Some(edited.clone()),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(
        updated.body["fields"]["visuals"],
        edited["fields"]["visuals"]
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!(
                "/api/v1/admin/services/{}",
                saved.body["id"].as_str().unwrap()
            ),
            Some(&token),
            Some(edited)
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn service_visuals_reject_wrong_purpose_posters_and_catalog_requires_service_permission(
    pool: PgPool,
) {
    let (_state, app, token, storage) = setup(pool).await;
    let signed = sign(&app, &token, "blog_cover").await;
    upload(&storage, &signed, webp(), "image/webp");
    let poster = complete(&app, &token, &signed).await;
    let seeds: Vec<Value> =
        serde_json::from_str(include_str!("../examples/services.json")).unwrap();
    let mut input = seeds[0].clone();
    input["assetIds"] = json!([poster.body["assetId"]]);
    input["fields"]["visuals"] =
        json!({"hero":{"type":"image","src":poster.body["publicUrl"],"alt":"Wrong purpose image"}});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/services",
            Some(&token),
            Some(input)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let registered = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({"name":"Reader","email":"reader@test.invalid","password":PASSWORD})),
    )
    .await;
    assert_eq!(registered.status, StatusCode::CREATED);
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"reader@test.invalid","password":PASSWORD})),
    )
    .await;
    let reader = login.body["accessToken"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/shaders", Some(reader), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn existing_service_images_can_be_promoted_to_shader_posters_without_backfill(pool: PgPool) {
    let (_state, app, token, _storage) = setup(pool).await;
    let seeds: Vec<Value> =
        serde_json::from_str(include_str!("../examples/services.json")).unwrap();
    let mut input = seeds[0].clone();
    input["fields"]["media"] = json!([{"slot":"hero","kind":"image","url":"/uploads/old-hero.webp","alt":"Existing hero","poster":"","caption":""}]);
    let saved = call(
        &app,
        "POST",
        "/api/v1/admin/services",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    let manifest = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(manifest.status, StatusCode::OK, "{}", manifest.body);
    let entry = &manifest.body["shaders"][0];
    let activated = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/shaders/{}/activation",
            entry["id"].as_str().unwrap()
        ),
        Some(&token),
        Some(json!({"version":entry["version"],"contract":entry["contract"],"active":true})),
    )
    .await;
    assert_eq!(activated.status, StatusCode::OK, "{}", activated.body);
    input["version"] = saved.body["version"].clone();
    input["fields"]["visuals"] = json!({"hero":{"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"poster":"/uploads/old-hero.webp","alt":"Existing hero as poster"}});
    let updated = call(
        &app,
        "PUT",
        &format!(
            "/api/v1/admin/services/{}",
            saved.body["id"].as_str().unwrap()
        ),
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.body["fields"]["media"], input["fields"]["media"]);
    assert_eq!(
        updated.body["fields"]["visuals"],
        input["fields"]["visuals"]
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn shader_activation_persists_enforces_permissions_and_preserves_existing_pages(
    pool: PgPool,
) {
    let (state, app, token, _storage) = setup(pool).await;
    let catalog = call(&app, "GET", "/api/v1/admin/shaders", Some(&token), None).await;
    assert_eq!(catalog.status, StatusCode::OK);
    assert_eq!(catalog.body["canManage"], true);
    let entry = catalog.body["shaders"][0].clone();
    let path = format!(
        "/api/v1/admin/shaders/{}/activation",
        entry["id"].as_str().unwrap()
    );
    let activation =
        |active| json!({"version":entry["version"],"contract":entry["contract"],"active":active});
    let mut input: Value =
        serde_json::from_str::<Vec<Value>>(include_str!("../examples/services.json"))
            .unwrap()
            .remove(0);
    input["fields"]["media"] = json!([{"slot":"hero","kind":"image","url":"/old-poster.webp","alt":"Existing poster","poster":"","caption":""}]);
    let saved = call(
        &app,
        "POST",
        "/api/v1/admin/services",
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    let service_path = format!(
        "/api/v1/admin/services/{}",
        saved.body["id"].as_str().unwrap()
    );
    let disabled = call(&app, "PUT", &path, Some(&token), Some(activation(false))).await;
    assert_eq!(disabled.status, StatusCode::OK, "{}", disabled.body);
    assert_eq!(disabled.body["shaders"][0]["active"], false);
    let recreated = create_router(state.clone()).unwrap();
    let reread = call(
        &recreated,
        "GET",
        "/api/v1/admin/shaders",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(reread.body["shaders"][0]["active"], false);
    assert_eq!(
        call(&app, "PUT", &path, None, Some(activation(true)))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let mut stale = activation(true);
    stale["contract"] = json!("0".repeat(64));
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(stale))
            .await
            .status,
        StatusCode::CONFLICT
    );
    let mut stale = activation(true);
    stale["version"] = json!(999);
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(stale))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/shaders/unconfigured-draft/activation",
            Some(&token),
            Some(activation(true))
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let mut executable = activation(true);
    executable["source"] = json!("alert(1)");
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(executable))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let registered = call(
        &app,
        "POST",
        "/api/v1/auth/register",
        None,
        Some(json!({"name":"Editor","email":"shader-editor@test.invalid","password":PASSWORD})),
    )
    .await;
    assert_eq!(registered.status, StatusCode::CREATED);
    sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT users.id,roles.id FROM users CROSS JOIN roles WHERE users.email='shader-editor@test.invalid' AND roles.name='editor' ON CONFLICT DO NOTHING").execute(&state.pool).await.unwrap();
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"shader-editor@test.invalid","password":PASSWORD})),
    )
    .await;
    let editor_token = login.body["accessToken"].as_str().unwrap();
    let editor_catalog = call(
        &app,
        "GET",
        "/api/v1/admin/shaders",
        Some(editor_token),
        None,
    )
    .await;
    assert_eq!(editor_catalog.status, StatusCode::OK);
    assert_eq!(editor_catalog.body["canManage"], false);
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            Some(editor_token),
            Some(activation(true))
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    input["version"] = saved.body["version"].clone();
    let visual = json!({"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"poster":"/old-poster.webp","alt":"Approved shader poster"});
    input["fields"]["visuals"] = json!({"hero":visual});
    assert_eq!(
        call(
            &app,
            "PUT",
            &service_path,
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for _attempt in 0..2 {
        assert_eq!(
            call(&app, "PUT", &path, Some(&token), Some(activation(true)))
                .await
                .status,
            StatusCode::OK
        );
    }
    let attached = call(
        &app,
        "PUT",
        &service_path,
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(attached.status, StatusCode::OK, "{}", attached.body);
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(activation(false)))
            .await
            .status,
        StatusCode::OK
    );
    input["version"] = attached.body["version"].clone();
    input["title"] = json!("Existing shader remains editable");
    let preserved = call(
        &app,
        "PUT",
        &service_path,
        Some(&token),
        Some(input.clone()),
    )
    .await;
    assert_eq!(preserved.status, StatusCode::OK, "{}", preserved.body);
    let public = call(
        &app,
        "GET",
        &format!("/api/v1/services/{}", input["slug"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.body["visuals"]["hero"], visual);
    input["version"] = preserved.body["version"].clone();
    input["fields"]["visuals"]["overview"] = visual;
    assert_eq!(
        call(
            &app,
            "PUT",
            &service_path,
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "PUT", &path, Some(&token), Some(activation(true)))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "PUT", &service_path, Some(&token), Some(input))
            .await
            .status,
        StatusCode::OK
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM shader_activations")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn media_requires_auth_verified_assets_and_compatible_purposes(pool: PgPool) {
    let (state, app, token, storage) = setup(pool).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/uploads/presign",
            None,
            Some(json!({}))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let signed = sign(&app, &token, "blog_cover").await;
    assert!(!signed["uploadUrl"].as_str().unwrap().contains("source.png"));
    assert_eq!(
        complete(&app, &token, &signed).await.status,
        StatusCode::CONFLICT
    );
    upload(&storage, &signed, webp(), "image/webp");
    let first = complete(&app, &token, &signed).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let again = complete(&app, &token, &signed).await;
    assert_eq!(first.body["assetId"], again.body["assetId"]);
    assert_eq!(storage.copies.load(Ordering::SeqCst), 1);
    let mut guessed = blog();
    guessed["fields"]["cover"] = json!("https://media.test/unverified.webp");
    guessed["fields"]["socialImage"] = json!("https://media.test/unverified.webp");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(&token),
            Some(guessed)
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut input = blog();
    input["fields"]["cover"] = first.body["publicUrl"].clone();
    input["fields"]["socialImage"] = first.body["publicUrl"].clone();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(&token),
            Some(input.clone())
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    input["assetIds"] = json!([first.body["assetId"]]);
    let saved = call(
        &app,
        "POST",
        "/api/v1/admin/blogs",
        Some(&token),
        Some(input),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    assert_eq!(saved.body["assetIds"], json!([first.body["assetId"]]));
    let mut wrong = project();
    wrong["assetIds"] = json!([first.body["assetId"]]);
    wrong["fields"]["image"] = first.body["publicUrl"].clone();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/projects",
            Some(&token),
            Some(wrong)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content_media_assets")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn media_copy_and_database_failures_are_retryable_without_duplicate_publication(
    pool: PgPool,
) {
    let (state, app, token, storage) = setup(pool).await;
    let signed = sign(&app, &token, "project_cover").await;
    upload(&storage, &signed, webp(), "image/webp");
    storage.fail_copy.store(true, Ordering::SeqCst);
    assert_eq!(
        complete(&app, &token, &signed).await.status,
        StatusCode::BAD_GATEWAY
    );
    storage.fail_copy.store(false, Ordering::SeqCst);
    sqlx::raw_sql("CREATE FUNCTION fail_asset_insert() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test DB failure'; END $$; CREATE TRIGGER fail_asset BEFORE INSERT ON media_assets FOR EACH ROW EXECUTE FUNCTION fail_asset_insert();").execute(&state.pool).await.unwrap();
    assert_eq!(
        complete(&app, &token, &signed).await.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(storage.copies.load(Ordering::SeqCst), 1);
    sqlx::raw_sql("DROP TRIGGER fail_asset ON media_assets; DROP FUNCTION fail_asset_insert();")
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE image_uploads SET expires_at=now()-interval '11 minutes' WHERE id=$1")
        .bind(Uuid::parse_str(signed["uploadId"].as_str().unwrap()).unwrap())
        .execute(&state.pool)
        .await
        .unwrap();
    state.media.cleanup(&state.pool).await.unwrap();
    assert_eq!(complete(&app, &token, &signed).await.status, StatusCode::OK);
    assert_eq!(storage.copies.load(Ordering::SeqCst), 1);
    let signed = sign(&app, &token, "project_cover").await;
    upload(&storage, &signed, webp(), "image/webp");
    let (first, second) = tokio::join!(
        complete(&app, &token, &signed),
        complete(&app, &token, &signed)
    );
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(second.status, StatusCode::OK, "{}", second.body);
    assert_eq!(first.body["assetId"], second.body["assetId"]);
    assert_eq!(storage.copies.load(Ordering::SeqCst), 2);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn media_invalid_bytes_mime_dimensions_and_expired_sessions_never_publish(pool: PgPool) {
    let (state, app, token, storage) = setup(pool).await;
    for (bytes, content_type) in [
        (webp(), "image/png"),
        (vec![0; webp().len()], "image/webp"),
        (webp()[..20].to_vec(), "image/webp"),
    ] {
        let signed = sign(&app, &token, "case_study_cover").await;
        upload(&storage, &signed, bytes, content_type);
        assert_eq!(
            complete(&app, &token, &signed).await.status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let signed = sign(&app, &token, "service_image").await;
    sqlx::query("UPDATE image_uploads SET expected_width=32,expected_height=18 WHERE id=$1")
        .bind(Uuid::parse_str(signed["uploadId"].as_str().unwrap()).unwrap())
        .execute(&state.pool)
        .await
        .unwrap();
    upload(&storage, &signed, webp(), "image/webp");
    assert_eq!(
        complete(&app, &token, &signed).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let signed = sign(&app, &token, "service_image").await;
    upload(&storage, &signed, webp(), "image/webp");
    let id = Uuid::parse_str(signed["uploadId"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE image_uploads SET expires_at=now()-interval '1 minute' WHERE id=$1")
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(
        complete(&app, &token, &signed).await.status,
        StatusCode::GONE
    );
    let unknown_owner = Uuid::new_v4();
    assert!(matches!(
        state.media.complete(&state.pool, id, unknown_owner).await,
        Err(AppError::Forbidden)
    ));
    assert_eq!(storage.copies.load(Ordering::SeqCst), 0);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn media_signing_enforces_payload_pending_and_per_actor_rate_limits(pool: PgPool) {
    let (state, app, token, _storage) = setup(pool).await;
    for mutation in [
        json!({"contentType":"image/png"}),
        json!({"sizeBytes":2097153}),
        json!({"width":99999}),
        json!({"purpose":"arbitrary_bucket"}),
        json!({"key":"chosen/by/client"}),
    ] {
        let mut input = json!({"purpose":"blog_cover","originalName":"photo.png","contentType":"image/webp","sizeBytes":400,"width":1920,"height":1080});
        input
            .as_object_mut()
            .unwrap()
            .extend(mutation.as_object().unwrap().clone());
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/admin/uploads/presign",
                Some(&token),
                Some(input)
            )
            .await
            .status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    for _attempt in 0..20 {
        sign(&app, &token, "service_image").await;
    }
    let metadata = json!({"purpose":"service_image","originalName":"photo.png","contentType":"image/webp","sizeBytes":400,"width":16,"height":9});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/uploads/presign",
            Some(&token),
            Some(metadata.clone())
        )
        .await
        .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("UPDATE image_uploads SET status='abandoned'")
        .execute(&state.pool)
        .await
        .unwrap();
    for _attempt in 0..10 {
        let signed = sign(&app, &token, "service_image").await;
        let path = format!(
            "/api/v1/admin/uploads/{}",
            signed["uploadId"].as_str().unwrap()
        );
        assert_eq!(
            call(&app, "DELETE", &path, Some(&token), None).await.status,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/uploads/presign",
            Some(&token),
            Some(metadata)
        )
        .await
        .status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL with CREATEDB; run in CI"]
async fn media_cleanup_protects_referenced_assets_and_retries_failed_orphan_deletion(pool: PgPool) {
    let (state, app, token, storage) = setup(pool).await;
    let signed = sign(&app, &token, "blog_cover").await;
    upload(&storage, &signed, webp(), "image/webp");
    let asset = complete(&app, &token, &signed).await.body;
    let mut input = blog();
    input["fields"]["cover"] = asset["publicUrl"].clone();
    input["fields"]["socialImage"] = asset["publicUrl"].clone();
    input["assetIds"] = json!([asset["assetId"]]);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/blogs",
            Some(&token),
            Some(input)
        )
        .await
        .status,
        StatusCode::CREATED
    );
    sqlx::query("UPDATE media_assets SET unreferenced_at=now()-interval '8 days'")
        .execute(&state.pool)
        .await
        .unwrap();
    state.media.cleanup(&state.pool).await.unwrap();
    assert!(storage
        .objects
        .lock()
        .unwrap()
        .contains_key(&(true, asset["key"].as_str().unwrap().into())));
    let orphan = sign(&app, &token, "service_image").await;
    upload(&storage, &orphan, webp(), "image/webp");
    let orphan = complete(&app, &token, &orphan).await.body;
    let orphan_id = Uuid::parse_str(orphan["assetId"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE media_assets SET unreferenced_at=now()-interval '8 days' WHERE id=$1")
        .bind(orphan_id)
        .execute(&state.pool)
        .await
        .unwrap();
    storage.fail_delete.store(true, Ordering::SeqCst);
    assert!(state.media.cleanup(&state.pool).await.is_err());
    let status: String = sqlx::query_scalar("SELECT status FROM media_assets WHERE id=$1")
        .bind(orphan_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(status, "deleting");
    storage.fail_delete.store(false, Ordering::SeqCst);
    state.media.cleanup(&state.pool).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM media_assets WHERE id=$1")
        .bind(orphan_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(status, "deleted");
}
