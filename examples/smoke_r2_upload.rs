use image::{codecs::webp::WebPEncoder, ExtendedColorType};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, StatusCode,
};
use serde_json::{json, Value};
use std::{env, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if env::var("R2_SMOKE_TEST").as_deref() != Ok("true") {
        return Err("Set R2_SMOKE_TEST=true to deliberately create a test image in the configured R2 buckets".into());
    }
    let origin = env::var("R2_SMOKE_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let parsed = url::Url::parse(&origin)?;
    if parsed.scheme() != "http"
        || parsed.host_str() != Some("127.0.0.1")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err("The smoke test API must be an HTTP loopback origin".into());
    }
    let token = env::var("R2_SMOKE_ACCESS_TOKEN")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes).encode(
        &[128; 16 * 9 * 4],
        16,
        9,
        ExtendedColorType::Rgba8,
    )?;
    let response = client.post(format!("{origin}/api/v1/admin/uploads/presign")).bearer_auth(&token)
        .json(&json!({"purpose":"service_image","originalName":"r2-smoke-test.webp","contentType":"image/webp","sizeBytes":bytes.len(),"width":16,"height":9}))
        .send().await.map_err(|_| "API signing request failed")?;
    if response.status() != StatusCode::CREATED {
        return Err("API signing rejected the smoke test".into());
    }
    let signed: Value = response.json().await?;
    let destination = signed["uploadUrl"].as_str().ok_or("Missing signed URL")?;
    let host = url::Url::parse(destination).map_err(|_| "Invalid signed destination")?;
    if host.scheme() != "https"
        || !host
            .host_str()
            .is_some_and(|name| name.ends_with(".r2.cloudflarestorage.com"))
    {
        return Err("Signed URL is not an R2 S3 endpoint".into());
    }
    let mut headers = HeaderMap::new();
    for (name, value) in signed["headers"]
        .as_object()
        .ok_or("Missing signed headers")?
    {
        if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("cookie") {
            return Err("Unexpected credential header".into());
        }
        headers.insert(
            HeaderName::from_bytes(name.as_bytes())?,
            HeaderValue::from_str(value.as_str().ok_or("Invalid signed header")?)?,
        );
    }
    let uploaded = client
        .put(destination)
        .headers(headers.clone())
        .body(bytes.clone())
        .send()
        .await
        .map_err(|_| "Direct R2 PUT failed")?;
    if !uploaded.status().is_success() {
        return Err("Direct R2 PUT was rejected".into());
    }
    let replay = client
        .put(destination)
        .headers(headers)
        .body(bytes.clone())
        .send()
        .await
        .map_err(|_| "R2 replay check failed")?;
    if replay.status() != StatusCode::PRECONDITION_FAILED {
        return Err("Conditional R2 PUT did not reject overwrite with HTTP 412".into());
    }
    let id = signed["uploadId"].as_str().ok_or("Missing upload ID")?;
    let mut canonical: Option<Value> = None;
    for _attempt in 0..2 {
        let response = client
            .post(format!("{origin}/api/v1/admin/uploads/{id}/complete"))
            .bearer_auth(&token)
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| "Completion request failed")?;
        if !response.status().is_success() {
            return Err("R2 verification/copy or database completion failed".into());
        }
        let asset: Value = response.json().await?;
        if asset["status"] != "ready"
            || canonical
                .as_ref()
                .is_some_and(|previous| previous["assetId"] != asset["assetId"])
        {
            return Err("Completion was not ready or idempotent".into());
        }
        canonical = Some(asset);
    }
    let asset = canonical.ok_or("Missing ready asset")?;
    let public = client
        .get(asset["publicUrl"].as_str().ok_or("Missing canonical URL")?)
        .send()
        .await
        .map_err(|_| "Public R2 GET failed")?;
    if !public.status().is_success()
        || public
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            != Some("image/webp")
        || !public
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("immutable"))
    {
        return Err("Public object MIME, cache metadata or response status was invalid".into());
    }
    if public
        .bytes()
        .await
        .map_err(|_| "Public object read failed")?
        .as_ref()
        != bytes.as_slice()
    {
        return Err("Published WebP bytes changed".into());
    }
    println!("PASS: real R2 PUT, overwrite rejection, verified copy, idempotent completion, public WebP/cache metadata and bytes");
    Ok(())
}
