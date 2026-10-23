use crate::common::{
    error::{AppError, AppResult},
    validation::{self, escape},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceMedia {
    pub slot: String,
    pub kind: String,
    pub url: String,
    pub alt: String,
    #[serde(default)]
    pub poster: String,
    #[serde(default)]
    pub caption: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceFields {
    pub category: String,
    pub artwork: String,
    pub enabled: bool,
    pub nav_enabled: bool,
    pub nav_title: String,
    pub nav_description: String,
    pub nav_group: String,
    pub nav_order: i32,
    pub nav_visual: String,
    pub nav_popular: bool,
    pub nav_ai: bool,
    pub page_content: Value,
    #[serde(default)]
    pub media: Vec<ServiceMedia>,
}

pub fn page_schema() -> AppResult<Value> {
    serde_json::from_str(include_str!(
        "../../../../examples/service-content.schema.json"
    ))
    .map_err(|_| AppError::Internal)
}

pub fn validate_page(value: &Value) -> AppResult<()> {
    validate_page_value(value, false)
}

pub fn validate_existing_page(value: &Value) -> AppResult<()> {
    validate_page_value(value, true)
}

fn validate_page_value(value: &Value, allow_empty: bool) -> AppResult<()> {
    fn check(value: &Value, schema: &Value, allow_empty: bool) -> AppResult<()> {
        match schema["type"].as_str() {
            Some("string") => validation::text(
                value
                    .as_str()
                    .ok_or_else(|| AppError::Invalid("Service content must contain text".into()))?,
                "service text",
                if allow_empty { 0 } else { 1 },
                20_000,
            )?,
            Some("array") => {
                let values = value.as_array().ok_or_else(|| {
                    AppError::Invalid("Service content must contain lists".into())
                })?;
                if values.len() > 30 {
                    return Err(AppError::Invalid(
                        "At most 30 service list entries are supported".into(),
                    ));
                }
                for value in values {
                    check(value, &schema["items"], allow_empty)?;
                }
            }
            Some("object") => {
                let values = value
                    .as_object()
                    .ok_or_else(|| AppError::Invalid("Invalid service section".into()))?;
                let properties = schema["properties"].as_object().ok_or(AppError::Internal)?;
                if values.len() != properties.len() {
                    return Err(AppError::Invalid(
                        "Service sections must match the content schema".into(),
                    ));
                }
                for (key, schema) in properties {
                    check(
                        values.get(key).ok_or_else(|| {
                            AppError::Invalid(format!("Missing service field: {key}"))
                        })?,
                        schema,
                        allow_empty,
                    )?;
                }
            }
            _ => return Err(AppError::Internal),
        }
        Ok(())
    }
    check(value, &page_schema()?, allow_empty)?;
    if value["process"]
        .as_array()
        .is_none_or(|items| items.len() != 5)
    {
        return Err(AppError::Invalid(
            "Provide all five service process phases".into(),
        ));
    }
    for name in ["related", "projectSlugs"] {
        for slug in value[name].as_array().ok_or(AppError::Internal)? {
            validation::slug(slug.as_str().ok_or(AppError::Internal)?)?;
        }
    }
    Ok(())
}

pub fn validate(fields: &ServiceFields) -> AppResult<()> {
    for (name, value, maximum) in [
        ("category", &fields.category, 100),
        ("navigation title", &fields.nav_title, 200),
        ("navigation description", &fields.nav_description, 1000),
        ("navigation group", &fields.nav_group, 100),
    ] {
        validation::text(value, name, 1, maximum)?;
    }
    if !(0..=10000).contains(&fields.nav_order)
        || !matches!(
            fields.artwork.as_str(),
            "web" | "software" | "cloud" | "data" | "automation" | "systems" | "growth"
        )
        || !matches!(
            fields.nav_visual.as_str(),
            "orbit" | "cube" | "signal" | "device" | "grid"
        )
    {
        return Err(AppError::Invalid(
            "Invalid service artwork or navigation order".into(),
        ));
    }
    validate_page(&fields.page_content)?;
    let mut slots = HashSet::new();
    if fields.media.len() > 6 {
        return Err(AppError::Invalid(
            "At most six service media slots are supported".into(),
        ));
    }
    for media in &fields.media {
        if !matches!(
            media.slot.as_str(),
            "hero" | "overview" | "spotlight" | "article-1" | "article-2" | "article-3"
        ) || !slots.insert(&media.slot)
            || !matches!(media.kind.as_str(), "image" | "video")
        {
            return Err(AppError::Invalid(
                "Invalid or duplicate service media slot".into(),
            ));
        }
        validation::url(&media.url, "media URL", true, false)?;
        validation::url(&media.poster, "video poster", true, true)?;
        validation::text(&media.alt, "media alt text", 1, 500)?;
        validation::text(&media.caption, "media caption", 0, 1000)?;
    }
    Ok(())
}

pub fn render(fields: &ServiceFields) -> String {
    fn render_value(value: &Value, key: &str) -> String {
        match value {
            Value::String(text) => {
                if key == "title" || key == "faqTitle" {
                    format!("<h2>{}</h2>", escape(text))
                } else {
                    format!("<p>{}</p>", escape(text))
                }
            }
            Value::Array(values) => values
                .iter()
                .map(|value| render_value(value, key))
                .collect(),
            Value::Object(values) => values
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "related" | "projectSlugs"))
                .map(|(key, value)| render_value(value, key))
                .collect(),
            _ => String::new(),
        }
    }
    render_value(&fields.page_content, "")
}
