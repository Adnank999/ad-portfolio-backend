use super::{
    dto::{ContentWrite, Kind},
    service,
};
use serde_json::{json, Value};

fn project() -> Value {
    serde_json::from_str(include_str!("../../../../examples/project.json")).unwrap()
}

fn prepare(value: Value) -> Result<ContentWrite, crate::common::error::AppError> {
    let mut input: ContentWrite = serde_json::from_value(value).unwrap();
    service::prepare(Kind::Project, &mut input)?;
    Ok(input)
}

#[test]
fn preserves_cover_and_ordered_gallery_and_defaults_legacy_projects() {
    let mut value = project();
    value["fields"].as_object_mut().unwrap().remove("gallery");
    let cover = value["fields"]["image"].clone();
    assert_eq!(prepare(value.clone()).unwrap().fields["gallery"], json!([]));
    value["fields"]["gallery"] = json!([
        {"url":"https://cdn.example.com/search.webp","alt":"Hotel search results","caption":"Search flow"},
        {"url":"/images/checkout.webp","alt":"Payment checkout"}
    ]);
    let prepared = prepare(value).unwrap();
    assert_eq!(prepared.fields["image"], cover);
    assert_eq!(prepared.fields["gallery"][0]["caption"], "Search flow");
    assert_eq!(
        prepared.fields["gallery"][1]["url"],
        "/images/checkout.webp"
    );
    assert_eq!(prepared.fields["gallery"][1]["caption"], "");
    let mut value = project();
    value["fields"]["gallery"] = json!([]);
    assert!(prepare(value).is_ok());
}

#[test]
fn rejects_unsafe_urls_invalid_metadata_duplicates_and_excessive_gallery_size() {
    let valid = json!({"url":"/images/screen.webp","alt":"Project dashboard","caption":""});
    let mut invalid_cases = Vec::new();
    for url in [
        "javascript:alert(1)",
        "data:image/svg+xml,test",
        "//evil.test/img",
        "https://user:password@cdn.test/image",
        "",
    ] {
        let mut image = valid.clone();
        image["url"] = json!(url);
        invalid_cases.push(json!([image]));
    }
    for (field, text) in [
        ("alt", "".to_string()),
        ("alt", "a".repeat(501)),
        ("caption", "a".repeat(1001)),
    ] {
        let mut image = valid.clone();
        image[field] = json!(text);
        invalid_cases.push(json!([image]));
    }
    invalid_cases.push(json!([valid.clone(), valid]));
    invalid_cases.push(json!((0..21)
        .map(|index| json!({"url":format!("/images/{index}.webp"),"alt":"Dashboard"}))
        .collect::<Vec<_>>()));
    for gallery in invalid_cases {
        let mut value = project();
        value["fields"]["gallery"] = gallery;
        assert!(prepare(value).is_err());
    }
    for gallery in [
        json!([{"url":"/images/a.webp"}]),
        json!([{"url":"/images/a.webp","alt":"Dashboard","unsupported":true}]),
        json!("/images/a.webp"),
    ] {
        let mut value = project();
        value["fields"]["gallery"] = gallery;
        let mut input: ContentWrite = serde_json::from_value(value).unwrap();
        assert!(service::prepare(Kind::Project, &mut input).is_err());
    }
}
