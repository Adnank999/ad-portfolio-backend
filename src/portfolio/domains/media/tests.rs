use super::{
    config::R2Config,
    policy::Purpose,
    storage::{ObjectStorage, R2Storage},
    validation::verify_webp,
};
use image::{codecs::webp::WebPEncoder, ExtendedColorType};

pub fn config() -> R2Config {
    R2Config {
        account_id: "a".repeat(32),
        access_key_id: "test-access-key".into(),
        secret_access_key: "test-secret-key".into(),
        staging_bucket: "test-private-staging".into(),
        public_bucket: "test-public-media".into(),
        public_base_url: "https://media.test".into(),
        ttl_seconds: 300,
    }
}

#[test]
fn policies_reject_invalid_dimensions_sizes_and_wrong_cover_ratios() {
    for purpose in [
        Purpose::BlogCover,
        Purpose::ProjectCover,
        Purpose::CaseStudyCover,
    ] {
        assert!(purpose.validate(400, 1920, 1080).is_ok());
        assert!(purpose.validate(400, 1000, 1000).is_err());
    }
    for (size, width, height) in [
        (0, 100, 100),
        (2 * 1024 * 1024 + 1, 100, 100),
        (100, 0, 100),
        (100, 2000, 100),
    ] {
        assert!(Purpose::ServiceImage.validate(size, width, height).is_err());
    }
}

#[test]
fn real_webp_decoder_rejects_malformed_truncated_and_mismatched_images() {
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes)
        .encode(&[255; 16 * 9 * 4], 16, 9, ExtendedColorType::Rgba8)
        .unwrap();
    assert!(verify_webp(&bytes, Purpose::BlogCover, 16, 9).is_ok());
    assert!(verify_webp(&bytes, Purpose::BlogCover, 32, 18).is_err());
    assert!(verify_webp(&bytes[..20], Purpose::BlogCover, 16, 9).is_err());
    assert!(verify_webp(b"not a webp image", Purpose::BlogCover, 16, 9).is_err());
    bytes[0] = 0;
    assert!(verify_webp(&bytes, Purpose::BlogCover, 16, 9).is_err());
}

#[test]
fn webp_validation_accepts_browser_vp8_and_alpha_vp8x_and_rejects_animation() {
    fn container(chunks: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(chunks.len() as u32 + 4).to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(chunks);
        bytes
    }
    let lossy = hex::decode("56503820280000003001009d012a1000090000c01225a000037000fef85f4ffffec95bfff6797fffd92b7fd8f2000000").unwrap();
    assert!(verify_webp(&container(&lossy), Purpose::BlogCover, 16, 9).is_ok());
    let mut extended = b"VP8X".to_vec();
    extended.extend_from_slice(&10_u32.to_le_bytes());
    extended.extend_from_slice(&[16, 0, 0, 0, 15, 0, 0, 8, 0, 0]);
    extended.extend_from_slice(
        &hex::decode("414c504817000000010f30ff11118220dba606f7b73cc42022fa3f01f0950400").unwrap(),
    );
    extended.extend_from_slice(&hex::decode("565038203e0000007001009d012a1000090000c0122588007b00039000fefdc67bffff2510bfffc9b7fec87ffbe17fed01b555d9bffc41ff573ffdf0bff780dc4ae800000000").unwrap());
    assert!(verify_webp(&container(&extended), Purpose::BlogCover, 16, 9).is_ok());
    extended[8] |= 2;
    assert!(verify_webp(&container(&extended), Purpose::BlogCover, 16, 9).is_err());
}

#[test]
fn storage_configuration_requires_distinct_buckets_https_and_short_expiry() {
    let mut value = config();
    assert!(value.validate().is_ok());
    value.public_bucket = value.staging_bucket.clone();
    assert!(value.validate().is_err());
    value = config();
    value.public_base_url = "http://media.test".into();
    assert!(value.validate().is_err());
    value = config();
    value.ttl_seconds = 3600;
    assert!(value.validate().is_err());
}

#[tokio::test]
async fn actual_sdk_signs_write_once_put_with_browser_headers_without_network_access() {
    let storage = R2Storage::new(config());
    let (url, headers) = storage.presign("pending/actor/upload.webp").await.unwrap();
    let parsed = url::Url::parse(&url).unwrap();
    assert_eq!(
        parsed.host_str(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.r2.cloudflarestorage.com")
    );
    assert!(headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("content-type") && value == "image/webp"));
    assert!(headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("if-none-match") && value == "*"));
    assert!(!headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("authorization")));
    let signed_headers = parsed
        .query_pairs()
        .find(|(name, _)| name == "X-Amz-SignedHeaders")
        .unwrap()
        .1
        .into_owned();
    assert!(signed_headers.contains("if-none-match"));
    assert!(signed_headers.contains("content-type"));
    assert_eq!(headers.len(), 2);
}

#[tokio::test]
async fn signing_requires_the_specific_content_write_permission_before_storage_or_database_access()
{
    let service = super::service::MediaService::new(None).unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1/unused")
        .unwrap();
    let actor = crate::domains::auth::Principal {
        id: uuid::Uuid::new_v4(),
        session_id: uuid::Uuid::new_v4(),
        email: "editor@test.invalid".into(),
        name: "Editor".into(),
        headline: "Contributor".into(),
        roles: vec!["editor".into()],
        permissions: vec!["projects:write".into()],
    };
    let input = super::service::PresignInput {
        purpose: Purpose::BlogCover,
        original_name: "photo.png".into(),
        content_type: "image/webp".into(),
        size_bytes: 400,
        width: 1920,
        height: 1080,
    };
    assert!(matches!(
        service.presign(&pool, &actor, input).await,
        Err(crate::common::error::AppError::Forbidden)
    ));
}
