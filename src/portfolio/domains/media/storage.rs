use super::config::R2Config;
use crate::common::error::{AppError, AppResult};
use async_trait::async_trait;
use aws_sdk_s3::{
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    presigning::PresigningConfig,
    types::MetadataDirective,
    Client,
};
use std::{collections::BTreeMap, time::Duration};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct StoredObject {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub etag: String,
    pub upload_id: Option<String>,
}

#[async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn presign(&self, key: &str) -> AppResult<(String, BTreeMap<String, String>)>;
    async fn inspect(&self, public: bool, key: &str) -> AppResult<Option<StoredObject>>;
    async fn publish(
        &self,
        staging: &str,
        public: &str,
        etag: &str,
        upload_id: &str,
    ) -> AppResult<()>;
    async fn delete(&self, public: bool, key: &str) -> AppResult<()>;
}

pub struct R2Storage {
    pub client: Client,
    pub config: R2Config,
}

impl R2Storage {
    pub fn new(config: R2Config) -> Self {
        let sdk = aws_sdk_s3::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("auto"))
            .endpoint_url(format!(
                "https://{}.r2.cloudflarestorage.com",
                config.account_id
            ))
            .credentials_provider(Credentials::new(
                config.access_key_id.clone(),
                config.secret_access_key.clone(),
                None,
                None,
                "r2",
            ))
            .force_path_style(true)
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
            .build();
        Self {
            client: Client::from_conf(sdk),
            config,
        }
    }

    fn bucket(&self, public: bool) -> &str {
        if public {
            &self.config.public_bucket
        } else {
            &self.config.staging_bucket
        }
    }
}

#[async_trait]
impl ObjectStorage for R2Storage {
    async fn presign(&self, key: &str) -> AppResult<(String, BTreeMap<String, String>)> {
        let signed = self
            .client
            .put_object()
            .bucket(&self.config.staging_bucket)
            .key(key)
            .content_type("image/webp")
            .if_none_match("*")
            .presigned(
                PresigningConfig::expires_in(Duration::from_secs(self.config.ttl_seconds))
                    .map_err(|_| AppError::Internal)?,
            )
            .await
            .map_err(|_| AppError::StorageUnavailable)?;
        let headers: BTreeMap<String, String> = signed
            .headers()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("host"))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        if !headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("content-type") && value == "image/webp")
            || !headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("if-none-match") && value == "*")
        {
            return Err(AppError::Internal);
        }
        Ok((signed.uri().to_owned(), headers))
    }

    async fn inspect(&self, public: bool, key: &str) -> AppResult<Option<StoredObject>> {
        let head = match self
            .client
            .head_object()
            .bucket(self.bucket(public))
            .key(key)
            .send()
            .await
        {
            Ok(head) => head,
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|error| error.is_not_found()) =>
            {
                return Ok(None)
            }
            Err(_) => return Err(AppError::StorageUnavailable),
        };
        let length = head.content_length().unwrap_or_default();
        if length <= 0 || length > 2 * 1024 * 1024 || head.content_type() != Some("image/webp") {
            return Err(AppError::Invalid(
                "Stored image size or content type is invalid".into(),
            ));
        }
        let etag = head.e_tag().ok_or(AppError::StorageUnavailable)?.to_owned();
        let object = self
            .client
            .get_object()
            .bucket(self.bucket(public))
            .key(key)
            .if_match(&etag)
            .send()
            .await
            .map_err(|_| AppError::StorageUnavailable)?;
        let mut bytes = Vec::with_capacity(length as usize);
        object
            .body
            .into_async_read()
            .take(length as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AppError::StorageUnavailable)?;
        if bytes.len() as i64 != length {
            return Err(AppError::Invalid("Stored image length changed".into()));
        }
        Ok(Some(StoredObject {
            bytes,
            content_type: "image/webp".into(),
            etag,
            upload_id: head
                .metadata()
                .and_then(|metadata| metadata.get("upload-id"))
                .cloned(),
        }))
    }

    async fn publish(
        &self,
        staging: &str,
        public: &str,
        etag: &str,
        upload_id: &str,
    ) -> AppResult<()> {
        self.client
            .copy_object()
            .bucket(&self.config.public_bucket)
            .key(public)
            .copy_source(format!("{}/{}", self.config.staging_bucket, staging))
            .copy_source_if_match(etag)
            .metadata_directive(MetadataDirective::Replace)
            .metadata("upload-id", upload_id)
            .content_type("image/webp")
            .cache_control("public, max-age=31536000, immutable")
            .send()
            .await
            .map_err(|_| AppError::StorageUnavailable)?;
        Ok(())
    }

    async fn delete(&self, public: bool, key: &str) -> AppResult<()> {
        self.client
            .delete_object()
            .bucket(self.bucket(public))
            .key(key)
            .send()
            .await
            .map_err(|_| AppError::StorageUnavailable)?;
        Ok(())
    }
}
