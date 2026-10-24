use super::{
    config::R2Config,
    policy::Purpose,
    storage::{ObjectStorage, R2Storage},
    validation::verify_webp,
};
use crate::{
    common::error::{AppError, AppResult},
    domains::{
        auth::Principal,
        content::dto::{ContentWrite, Kind},
    },
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresignInput {
    pub purpose: Purpose,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresignResponse {
    pub upload_id: Uuid,
    pub method: &'static str,
    pub upload_url: String,
    pub headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    #[serde(rename = "assetId")]
    pub id: Uuid,
    pub status: String,
    pub public_url: String,
    #[serde(rename = "key")]
    pub public_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub width: i32,
    pub height: i32,
    #[serde(skip)]
    pub created_by: Uuid,
    pub purpose: String,
}

#[derive(FromRow)]
struct Upload {
    id: Uuid,
    requested_by: Uuid,
    purpose: String,
    staging_key: String,
    public_key: String,
    asset_id: Uuid,
    status: String,
    expected_size_bytes: i64,
    expected_width: i32,
    expected_height: i32,
    expires_at: DateTime<Utc>,
}

pub struct MediaService {
    pub config: Option<R2Config>,
    pub storage: Option<Arc<dyn ObjectStorage>>,
    workers: Arc<Semaphore>,
}

impl MediaService {
    pub fn new(config: Option<R2Config>) -> AppResult<Self> {
        if let Some(config) = &config {
            config.validate().map_err(|_| AppError::Unavailable)?;
        }
        let storage = config
            .clone()
            .map(|config| Arc::new(R2Storage::new(config)) as Arc<dyn ObjectStorage>);
        Ok(Self {
            config,
            storage,
            workers: Arc::new(Semaphore::new(2)),
        })
    }

    pub async fn presign(
        &self,
        pool: &PgPool,
        actor: &Principal,
        input: PresignInput,
    ) -> AppResult<PresignResponse> {
        actor.require(&format!("{}:write", input.purpose.kind()))?;
        let config = self.config.as_ref().ok_or(AppError::Unavailable)?;
        let storage = self.storage.as_ref().ok_or(AppError::Unavailable)?;
        if input.content_type != "image/webp"
            || input.original_name.is_empty()
            || input.original_name.chars().count() > 255
        {
            return Err(AppError::Invalid(
                "Only prepared WebP images with a valid original name may be signed".into(),
            ));
        }
        input
            .purpose
            .validate(input.size_bytes, input.width, input.height)?;
        let mut transaction = pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("media:{}", actor.id))
            .execute(&mut *transaction)
            .await?;
        let recent: i64 = sqlx::query_scalar("SELECT count(*) FROM image_uploads WHERE requested_by=$1 AND created_at>now()-interval '1 minute'")
            .bind(actor.id).fetch_one(&mut *transaction).await?;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM image_uploads WHERE requested_by=$1 AND status='pending' AND expires_at>now()")
            .bind(actor.id).fetch_one(&mut *transaction).await?;
        if recent >= 30 || pending >= 20 {
            return Err(AppError::UploadRateLimited);
        }
        let upload_id = Uuid::new_v4();
        let asset_id = Uuid::new_v4();
        let staging = format!("pending/{}/{upload_id}.webp", actor.id);
        let public = format!(
            "{}/{}/{}/{asset_id}.webp",
            input.purpose.kind(),
            input.purpose.as_str(),
            Utc::now().format("%Y/%m")
        );
        let expires_at = Utc::now() + Duration::seconds(config.ttl_seconds as i64);
        sqlx::query("INSERT INTO image_uploads(id,requested_by,purpose,staging_key,public_key,asset_id,expected_size_bytes,expected_width,expected_height,original_name,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
            .bind(upload_id).bind(actor.id).bind(input.purpose.as_str()).bind(&staging).bind(public).bind(asset_id)
            .bind(input.size_bytes).bind(input.width as i32).bind(input.height as i32).bind(input.original_name).bind(expires_at)
            .execute(&mut *transaction).await?;
        let (upload_url, headers) = storage.presign(&staging).await?;
        transaction.commit().await?;
        Ok(PresignResponse {
            upload_id,
            method: "PUT",
            upload_url,
            headers,
            expires_at,
        })
    }

    pub async fn complete(&self, pool: &PgPool, id: Uuid, actor: Uuid) -> AppResult<Asset> {
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::UploadRateLimited)?;
        let config = self.config.as_ref().ok_or(AppError::Unavailable)?;
        let storage = self.storage.as_ref().ok_or(AppError::Unavailable)?;
        let mut transaction = pool.begin().await?;
        let upload =
            sqlx::query_as::<_, Upload>("SELECT * FROM image_uploads WHERE id=$1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(AppError::NotFound)?;
        if upload.requested_by != actor {
            return Err(AppError::Forbidden);
        }
        if upload.status == "completed" {
            let asset =
                sqlx::query_as("SELECT * FROM media_assets WHERE upload_id=$1 AND status='ready'")
                    .bind(id)
                    .fetch_optional(&mut *transaction)
                    .await?
                    .ok_or(AppError::NotFound)?;
            transaction.commit().await?;
            return Ok(asset);
        }
        if upload.status != "pending" {
            return Err(AppError::Conflict("Upload is no longer pending".into()));
        }
        let (object, already_copied) = match self.inspect_upload(&upload).await {
            Ok(result) => result,
            Err(error @ AppError::Invalid(_)) => {
                sqlx::query(
                    "UPDATE image_uploads SET status='failed',updated_at=now() WHERE id=$1",
                )
                .bind(id)
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let purpose: Purpose = serde_json::from_value(Value::String(upload.purpose.clone()))
            .map_err(|_| AppError::Internal)?;
        let valid_size = object.bytes.len() as i64 == upload.expected_size_bytes
            && object.content_type == "image/webp";
        let width = upload.expected_width as u32;
        let height = upload.expected_height as u32;
        let etag = object.etag;
        let validation = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if !valid_size {
                return Err(AppError::Invalid(
                    "Stored image does not match the signed metadata".into(),
                ));
            }
            verify_webp(&object.bytes, purpose, width, height)
        })
        .await
        .map_err(|_| AppError::Internal)?;
        if let Err(error) = validation {
            sqlx::query("UPDATE image_uploads SET status='failed',updated_at=now() WHERE id=$1")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            return Err(error);
        }
        if !already_copied {
            storage
                .publish(
                    &upload.staging_key,
                    &upload.public_key,
                    &etag,
                    &id.to_string(),
                )
                .await?;
        }
        let public_url = format!(
            "{}/{}",
            config.public_base_url.trim_end_matches('/'),
            upload.public_key
        );
        sqlx::query("INSERT INTO media_assets(id,created_by,upload_id,purpose,public_key,public_url,content_type,size_bytes,width,height) VALUES($1,$2,$3,$4,$5,$6,'image/webp',$7,$8,$9) ON CONFLICT(upload_id) DO NOTHING")
            .bind(upload.asset_id).bind(actor).bind(id).bind(&upload.purpose).bind(&upload.public_key).bind(public_url)
            .bind(upload.expected_size_bytes).bind(upload.expected_width).bind(upload.expected_height).execute(&mut *transaction).await?;
        sqlx::query("UPDATE image_uploads SET status='completed',updated_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        let asset =
            sqlx::query_as("SELECT * FROM media_assets WHERE upload_id=$1 AND status='ready'")
                .bind(id)
                .fetch_one(&mut *transaction)
                .await?;
        transaction.commit().await?;
        tracing::info!(upload_id=%upload.id, "Image published");
        Ok(asset)
    }

    async fn inspect_upload(
        &self,
        upload: &Upload,
    ) -> AppResult<(super::storage::StoredObject, bool)> {
        let storage = self.storage.as_ref().ok_or(AppError::Unavailable)?;
        if let Some(object) = storage.inspect(true, &upload.public_key).await? {
            if object.upload_id.as_deref() != Some(&upload.id.to_string()) {
                return Err(AppError::StorageUnavailable);
            }
            return Ok((object, true));
        }
        if Utc::now() > upload.expires_at {
            return Err(AppError::UploadExpired);
        }
        let object = storage
            .inspect(false, &upload.staging_key)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("Upload bytes have not arrived; retry completion".into())
            })?;
        Ok((object, false))
    }

    pub async fn abandon(&self, pool: &PgPool, id: Uuid, actor: Uuid) -> AppResult<()> {
        let result = sqlx::query("UPDATE image_uploads SET status='abandoned',updated_at=now() WHERE id=$1 AND requested_by=$2 AND status='pending'")
            .bind(id).bind(actor).execute(pool).await?;
        if result.rows_affected() == 0 {
            return Err(AppError::Conflict("Upload cannot be abandoned".into()));
        }
        Ok(())
    }

    pub async fn cleanup(&self, pool: &PgPool) -> AppResult<()> {
        let storage = self.storage.as_ref().ok_or(AppError::Unavailable)?;
        let candidates = sqlx::query_as::<_, Upload>("SELECT * FROM image_uploads WHERE expires_at<now()-interval '10 minutes' AND staging_cleaned_at IS NULL ORDER BY expires_at LIMIT 50")
            .fetch_all(pool).await?;
        for upload in candidates {
            if upload.status == "pending" {
                match self.complete(pool, upload.id, upload.requested_by).await {
                    Ok(_) | Err(AppError::UploadExpired) | Err(AppError::Invalid(_)) => {}
                    Err(_) => continue,
                }
            }
            let mut transaction = pool.begin().await?;
            let current = sqlx::query_as::<_, Upload>(
                "SELECT * FROM image_uploads WHERE id=$1 FOR UPDATE SKIP LOCKED",
            )
            .bind(upload.id)
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(current) = current else {
                continue;
            };
            if current.status != "completed" {
                storage.delete(true, &current.public_key).await?;
                sqlx::query("UPDATE image_uploads SET status=CASE WHEN status='pending' THEN 'failed' ELSE status END,public_cleaned_at=now(),updated_at=now() WHERE id=$1")
                    .bind(current.id).execute(&mut *transaction).await?;
            }
            storage.delete(false, &current.staging_key).await?;
            sqlx::query(
                "UPDATE image_uploads SET staging_cleaned_at=now(),updated_at=now() WHERE id=$1",
            )
            .bind(current.id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
        }
        let orphans = sqlx::query_scalar::<_, Uuid>("SELECT id FROM media_assets a WHERE a.status IN ('ready','deleting') AND a.unreferenced_at<now()-interval '7 days' AND NOT EXISTS(SELECT 1 FROM content_media_assets r WHERE r.asset_id=a.id) ORDER BY a.id LIMIT 50")
            .fetch_all(pool).await?;
        for id in orphans {
            let mut transaction = pool.begin().await?;
            let asset = sqlx::query_as::<_, Asset>("SELECT * FROM media_assets a WHERE id=$1 AND a.status IN ('ready','deleting') AND a.unreferenced_at<now()-interval '7 days' AND NOT EXISTS(SELECT 1 FROM content_media_assets r WHERE r.asset_id=a.id) FOR UPDATE SKIP LOCKED")
                .bind(id).fetch_optional(&mut *transaction).await?;
            let Some(asset) = asset else {
                continue;
            };
            sqlx::query("UPDATE media_assets SET status='deleting' WHERE id=$1")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            storage.delete(true, &asset.public_key).await?;
            sqlx::query(
                "UPDATE media_assets SET status='deleted' WHERE id=$1 AND status='deleting'",
            )
            .bind(id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }
}

fn image_urls(value: &Value, output: &mut HashSet<String>) {
    match value {
        Value::String(text)
            if url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) =>
        {
            output.insert(text.clone());
        }
        Value::Object(object) => {
            for value in object.values() {
                image_urls(value, output);
            }
        }
        Value::Array(values) => {
            for value in values {
                image_urls(value, output);
            }
        }
        _ => {}
    }
}

pub async fn validate_references(
    transaction: &mut Transaction<'_, Postgres>,
    input: &ContentWrite,
    previous: Option<&crate::domains::content::repository::Record>,
    kind: Kind,
    actor: &Principal,
    public_base: Option<&str>,
) -> AppResult<Vec<Uuid>> {
    if input.asset_ids.len() > 100 {
        return Err(AppError::Invalid(
            "At most 100 media references are supported".into(),
        ));
    }
    let mut urls = HashSet::new();
    image_urls(&input.fields, &mut urls);
    if let Some(document) = &input.editor_content {
        image_urls(document, &mut urls);
    }
    image_urls(
        &serde_json::to_value(&input.seo).map_err(|_| AppError::Internal)?,
        &mut urls,
    );
    let url_list: Vec<String> = urls.into_iter().collect();
    let current = previous
        .map(|record| record.media_asset_ids.as_slice())
        .unwrap_or_default();
    let mut legacy_urls = HashSet::new();
    if let Some(record) = previous {
        image_urls(&record.fields, &mut legacy_urls);
        if let Some(document) = &record.editor_content {
            image_urls(document, &mut legacy_urls);
        }
        image_urls(&record.seo, &mut legacy_urls);
    }
    let assets = sqlx::query_as::<_, Asset>(
        "SELECT * FROM media_assets WHERE id=ANY($1) OR public_url=ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(
        input
            .asset_ids
            .iter()
            .chain(current)
            .copied()
            .collect::<Vec<_>>(),
    )
    .bind(&url_list)
    .fetch_all(&mut **transaction)
    .await?;
    let requested: HashSet<Uuid> = input.asset_ids.iter().chain(current).copied().collect();
    if requested
        .iter()
        .any(|id| !assets.iter().any(|asset| &asset.id == id))
    {
        return Err(AppError::Invalid("Unknown media asset".into()));
    }
    if let Some(base) = public_base {
        let base = url::Url::parse(base).map_err(|_| AppError::Internal)?;
        for url in &url_list {
            let managed_host = url::Url::parse(url).is_ok_and(|candidate| {
                candidate.host_str().map(|host| host.trim_end_matches('.'))
                    == base.host_str().map(|host| host.trim_end_matches('.'))
            });
            if managed_host
                && !legacy_urls.contains(url)
                && !assets
                    .iter()
                    .any(|asset| &asset.public_url == url && requested.contains(&asset.id))
            {
                return Err(AppError::Invalid(
                    "New R2 images require a verified media asset ID".into(),
                ));
            }
        }
    }
    let mut used = Vec::new();
    for asset in assets {
        if !url_list.contains(&asset.public_url) {
            continue;
        }
        let purpose: Purpose =
            serde_json::from_value(Value::String(asset.purpose)).map_err(|_| AppError::Internal)?;
        let mut allowed_urls = HashSet::new();
        match purpose {
            Purpose::BlogCover | Purpose::ProjectCover | Purpose::CaseStudyCover => {
                if let Some(value) =
                    input
                        .fields
                        .get(if kind == Kind::Blog { "cover" } else { "image" })
                {
                    image_urls(value, &mut allowed_urls);
                }
                if let Some(value) = input.fields.get("socialImage") {
                    image_urls(value, &mut allowed_urls);
                }
            }
            Purpose::ProjectGallery | Purpose::CaseStudyGallery => {
                if let Some(value) = input.fields.get("gallery") {
                    image_urls(value, &mut allowed_urls);
                }
            }
            Purpose::BlogInline | Purpose::ProjectInline | Purpose::CaseStudyInline => {
                if let Some(value) = &input.editor_content {
                    image_urls(value, &mut allowed_urls);
                }
            }
            Purpose::ServiceImage => {
                if let Some(value) = input.fields.get("media") {
                    image_urls(value, &mut allowed_urls);
                }
            }
        }
        image_urls(
            &serde_json::to_value(&input.seo).map_err(|_| AppError::Internal)?,
            &mut allowed_urls,
        );
        if !requested.contains(&asset.id)
            || asset.status != "ready"
            || purpose.kind() != kind.plural()
            || !allowed_urls.contains(&asset.public_url)
            || (asset.created_by != actor.id
                && !current.contains(&asset.id)
                && !actor
                    .roles
                    .iter()
                    .any(|role| role == "owner" || role == "admin"))
        {
            return Err(AppError::Forbidden);
        }
        used.push(asset.id);
    }
    Ok(used)
}

pub async fn save_references(
    transaction: &mut Transaction<'_, Postgres>,
    content: Uuid,
    previous: &[Uuid],
    used: &[Uuid],
) -> AppResult<()> {
    sqlx::query("DELETE FROM content_media_assets WHERE content_id=$1")
        .bind(content)
        .execute(&mut **transaction)
        .await?;
    for asset in used {
        sqlx::query("INSERT INTO content_media_assets(content_id,asset_id) VALUES($1,$2)")
            .bind(content)
            .bind(asset)
            .execute(&mut **transaction)
            .await?;
    }
    let affected: Vec<Uuid> = previous.iter().chain(used).copied().collect();
    sqlx::query("UPDATE media_assets a SET unreferenced_at=CASE WHEN EXISTS(SELECT 1 FROM content_media_assets r WHERE r.asset_id=a.id) THEN NULL ELSE COALESCE(a.unreferenced_at,now()) END WHERE a.id=ANY($1)")
        .bind(affected).execute(&mut **transaction).await?;
    Ok(())
}
