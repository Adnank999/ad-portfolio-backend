use super::{
    dto::{ContentWrite, Kind},
    service,
};
use serde_json::{json, Value};

fn seeds() -> Vec<Value> {
    serde_json::from_str(include_str!("../../../../examples/case-studies.json")).unwrap()
}

#[test]
fn every_existing_case_study_is_valid_and_keeps_its_media_and_faqs() {
    let records = seeds();
    assert_eq!(records.len(), 13);
    let mut slugs = std::collections::HashSet::new();
    for value in records {
        assert!(slugs.insert(value["slug"].as_str().unwrap().to_owned()));
        let mut input: ContentWrite = serde_json::from_value(value.clone()).unwrap();
        let (html, plain) = service::prepare(Kind::CaseStudy, &mut input).unwrap();
        assert_eq!(input.fields["steps"], value["fields"]["steps"]);
        assert!(!input.faqs.is_empty());
        assert!(html.contains("Frequently asked questions"));
        assert!(plain.len() > 100);
        assert!(!input.fields["image"].as_str().unwrap().ends_with(".svg"));
    }
}

#[test]
fn rejects_invalid_industries_labels_unsafe_media_and_gallery_duplicates() {
    let valid = seeds().remove(0);
    for (field, value) in [
        ("industry", json!("unknown")),
        ("studyType", json!("published")),
        ("image", json!("javascript:alert(1)")),
        ("imageAlt", json!("")),
        ("projectSlug", json!("../bad")),
    ] {
        let mut input = valid.clone();
        input["fields"][field] = value;
        let mut input: ContentWrite = serde_json::from_value(input).unwrap();
        assert!(service::prepare(Kind::CaseStudy, &mut input).is_err());
    }
    let mut duplicate = valid;
    let photo = json!({"url":"/uploads/screen.webp","alt":"Dashboard","caption":"Overview"});
    duplicate["fields"]["gallery"] = json!([photo, photo]);
    assert!(service::prepare(
        Kind::CaseStudy,
        &mut serde_json::from_value(duplicate).unwrap()
    )
    .is_err());
}

#[test]
fn validates_custom_json_ld_and_renders_untrusted_narrative_as_text() {
    let mut input = seeds().remove(0);
    input["fields"]["context"] = json!("<script>alert(1)</script>");
    input["seo"]["structuredData"] = json!({"@context":"https://schema.org","@type":"SoftwareApplication","name":"Verified application"});
    let mut valid: ContentWrite = serde_json::from_value(input.clone()).unwrap();
    let (html, _) = service::prepare(Kind::CaseStudy, &mut valid).unwrap();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    for invalid in [
        json!("not an object"),
        json!({"@context":"https://evil.test","@type":"Article"}),
        json!({"@context":"https://schema.org","@graph":[]}),
    ] {
        input["seo"]["structuredData"] = invalid;
        assert!(service::prepare(
            Kind::CaseStudy,
            &mut serde_json::from_value(input.clone()).unwrap()
        )
        .is_err());
    }
}
