use super::{
    dto::{ContentWrite, Kind},
    service,
};
use serde_json::{json, Value};

fn seeds() -> Vec<Value> {
    serde_json::from_str(include_str!("../../../../examples/services.json")).unwrap()
}

#[test]
fn all_thirteen_existing_services_preserve_sections_navigation_and_faqs() {
    let records = seeds();
    assert_eq!(records.len(), 13);
    let mut slugs = std::collections::HashSet::new();
    for record in records {
        assert!(slugs.insert(record["slug"].as_str().unwrap().to_owned()));
        let mut input: ContentWrite = serde_json::from_value(record.clone()).unwrap();
        let (html, text) = service::prepare(Kind::Service, &mut input).unwrap();
        assert_eq!(input.fields, record["fields"]);
        assert_eq!(input.faqs.len(), record["faqs"].as_array().unwrap().len());
        assert!(text.len() > 1000);
        assert!(html.contains("Frequently asked questions"));
    }
}

#[test]
fn validates_media_schema_switches_and_untrusted_copy() {
    let original = seeds().remove(0);
    let mut record = original.clone();
    record["fields"]["media"] = json!([{"slot":"overview","kind":"video","url":"/uploads/demo.mp4","alt":"A demonstration of the portal","poster":"/uploads/poster.webp","caption":"Portal walkthrough"}]);
    record["fields"]["pageContent"]["overview"]["text"] = json!("<script>alert(1)</script>");
    let (html, _) = service::prepare(
        Kind::Service,
        &mut serde_json::from_value(record.clone()).unwrap(),
    )
    .unwrap();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    for (key, value) in [
        ("kind", json!("iframe")),
        ("url", json!("javascript:alert(1)")),
        ("poster", json!("//evil.test/image.png")),
        ("alt", json!("")),
        ("slot", json!("unknown")),
    ] {
        let mut invalid = record.clone();
        invalid["fields"]["media"][0][key] = value;
        assert!(
            service::prepare(Kind::Service, &mut serde_json::from_value(invalid).unwrap()).is_err()
        );
    }
    let mut invalid = original;
    invalid["fields"]["pageContent"]["process"] = json!(["Incomplete"]);
    assert!(
        service::prepare(Kind::Service, &mut serde_json::from_value(invalid).unwrap()).is_err()
    );
    record["fields"]["enabled"] = json!("false");
    assert!(service::prepare(Kind::Service, &mut serde_json::from_value(record).unwrap()).is_err());
}
