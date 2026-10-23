use super::{dto::*, richtext};
use crate::{
    common::{
        error::{AppError, AppResult},
        validation,
    },
    domains::content::{dto::Faq, editor},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub fn empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(Value::Array(values)) => values.iter().all(|value| empty(Some(value))),
        Some(document) if document.get("type").and_then(Value::as_str) == Some("doc") => {
            if document.to_string().contains("\"image\"") {
                return false;
            }
            editor::render_document(document)
                .and_then(|html| editor::plain_text(&html))
                .map(|text| text.trim().is_empty())
                .unwrap_or(false)
        }
        Some(Value::Object(values)) => values.values().all(|value| empty(Some(value))),
        _ => false,
    }
}

pub fn request(input: &GenerateRequest) -> AppResult<()> {
    validation::text(&input.context, "context", 20, 20_000)?;
    validation::text(
        &input.additional_instructions,
        "additionalInstructions",
        0,
        4000,
    )?;
    validation::text(&input.language, "language", 2, 80)?;
    strings(&input.seo_keywords, 20, 100)?;
    if input.target_fields.is_empty() || input.target_fields.len() > Field::ALL.len() {
        return Err(AppError::Invalid(
            "Select at least one supported generation field".into(),
        ));
    }
    let mut seen = HashSet::new();
    for field in &input.target_fields {
        if !field.allowed(input.content_type) || !seen.insert(field.key()) {
            return Err(AppError::Invalid(
                "Unsupported or duplicate target field".into(),
            ));
        }
    }
    if !matches!(
        (&input.content_context, input.content_type),
        (ContentContext::Blog(_), ContentType::Blog)
            | (ContentContext::Project(_), ContentType::Project)
            | (ContentContext::CaseStudy(_), ContentType::CaseStudy)
            | (ContentContext::Service(_), ContentType::Service)
    ) {
        return Err(AppError::Invalid(
            "Content context must match contentType".into(),
        ));
    }
    let context = serde_json::to_value(&input.content_context).map_err(|_| AppError::Internal)?;
    bounded_context(&context)?;
    if let ContentContext::Project(context) = &input.content_context {
        if !context.year.is_empty()
            && (context.year.len() != 4 || !context.year.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(AppError::Invalid(
                "Project year must be four explicitly supplied digits".into(),
            ));
        }
    }
    for (name, value) in &input.existing_fields {
        let field = Field::ALL
            .iter()
            .find(|field| field.key() == name && field.allowed(input.content_type));
        let asset = match input.content_type {
            ContentType::Blog => name == "fields.cover",
            ContentType::Project => matches!(
                name.as_str(),
                "fields.image" | "fields.githubUrl" | "fields.liveUrl"
            ),
            ContentType::CaseStudy | ContentType::Service => name == "fields.image",
        };
        if field.is_none() && !asset {
            return Err(AppError::Invalid("Unsupported existing field".into()));
        }
        if empty(Some(value)) {
            continue;
        }
        if let Some(field) = field {
            match field {
                Field::PageContent => {
                    crate::domains::content::services::validate_existing_page(value)?;
                }
                Field::EditorContent => {
                    editor::render_document(value)?;
                }
                Field::Tags | Field::Features | Field::Technologies | Field::Lessons => {
                    let values: Vec<String> =
                        serde_json::from_value(value.clone()).map_err(|_| {
                            AppError::Invalid("Existing list fields must be string arrays".into())
                        })?;
                    strings(&values, 100, 1000)?;
                }
                Field::Faqs => {
                    let values: Vec<Faq> = serde_json::from_value(value.clone()).map_err(|_| {
                        AppError::Invalid("Existing FAQs must be question/answer objects".into())
                    })?;
                    if values.len() > 30 {
                        return Err(AppError::Invalid("Too many existing FAQs".into()));
                    }
                    for faq in values {
                        validation::text(&faq.question, "FAQ question", 0, 300)?;
                        validation::text(&faq.answer, "FAQ answer", 0, 5000)?;
                    }
                }
                _ => {
                    validation::text(
                        value.as_str().ok_or_else(|| {
                            AppError::Invalid("Existing text fields must be strings".into())
                        })?,
                        name,
                        0,
                        50_000,
                    )?;
                }
            }
        } else {
            validation::url(
                value.as_str().ok_or_else(|| {
                    AppError::Invalid("Existing asset references must be strings".into())
                })?,
                name,
                true,
                false,
            )?;
        }
    }
    if serde_json::to_vec(input)
        .map_err(|_| AppError::Internal)?
        .len()
        > 256_000
    {
        return Err(AppError::Invalid("Generation context is too large".into()));
    }
    Ok(())
}

fn bounded_context(value: &Value) -> AppResult<()> {
    match value {
        Value::String(text) => validation::text(text, "contentContext", 0, 5000)?,
        Value::Array(values) => {
            if values.len() > 50 {
                return Err(AppError::Invalid("Too many context items".into()));
            }
            for value in values {
                bounded_context(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                bounded_context(value)?;
            }
        }
        _ => {
            return Err(AppError::Invalid(
                "Context values must be text or lists".into(),
            ))
        }
    }
    Ok(())
}
fn strings(values: &[String], maximum: usize, text_maximum: usize) -> AppResult<()> {
    if values.len() > maximum {
        return Err(AppError::Invalid("Too many list items".into()));
    }
    for text in values {
        validation::text(text, "list item", 1, text_maximum)?;
    }
    Ok(())
}

pub fn facts(input: &GenerateRequest) -> String {
    format!(
        "{} {} {}",
        input.context,
        serde_json::to_string(&input.content_context).unwrap_or_default(),
        serde_json::to_string(&input.existing_fields).unwrap_or_default()
    )
    .to_lowercase()
}

pub fn normalize(field: Field, value: Value, input: &GenerateRequest) -> AppResult<Value> {
    fn invalid<Error>(_error: Error) -> AppError {
        AppError::AiInvalidOutput
    }
    let value = match field {
        Field::PageContent => {
            crate::domains::content::services::validate_page(&value).map_err(invalid)?;
            value
        }
        Field::EditorContent => richtext::document(value)?,
        Field::Tags | Field::Features | Field::Technologies | Field::Lessons => {
            let values: Vec<String> = serde_json::from_value(value).map_err(invalid)?;
            strings(
                &values,
                if field == Field::Features {
                    100
                } else if field == Field::Technologies {
                    40
                } else {
                    30
                },
                500,
            )
            .map_err(invalid)?;
            let mut seen = HashSet::new();
            let values: Vec<String> = values
                .into_iter()
                .map(|text| text.trim().to_owned())
                .filter(|text| seen.insert(text.to_lowercase()))
                .collect();
            if field == Field::Technologies
                && values.iter().any(|technology| {
                    let pattern = format!(
                        r"(?i)(?:^|[^a-z0-9]){}(?:$|[^a-z0-9])",
                        regex::escape(technology)
                    );
                    regex::Regex::new(&pattern)
                        .map(|pattern| !pattern.is_match(&facts(input)))
                        .unwrap_or(true)
                })
            {
                return Err(AppError::AiInvalidOutput);
            }
            json!(values)
        }
        Field::Faqs => {
            let values: Vec<Faq> = serde_json::from_value(value).map_err(invalid)?;
            if values.len() > 5 {
                return Err(AppError::AiInvalidOutput);
            }
            let mut seen: HashSet<String> = input
                .existing_fields
                .get("faqs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|faq| faq.get("question").and_then(Value::as_str))
                .map(|text| text.trim().to_lowercase())
                .collect();
            let mut output = Vec::new();
            for faq in values {
                validation::text(&faq.question, "FAQ question", 1, 300).map_err(invalid)?;
                validation::text(&faq.answer, "FAQ answer", 1, 5000).map_err(invalid)?;
                if seen.insert(faq.question.trim().to_lowercase()) {
                    output.push(json!({"question":faq.question.trim(),"answer":faq.answer.trim()}));
                }
            }
            json!(output)
        }
        _ => {
            let text = value.as_str().ok_or(AppError::AiInvalidOutput)?.trim();
            validation::text(text, field.key(), 1, field.maximum()).map_err(invalid)?;
            if field == Field::Slug {
                let pattern = regex::Regex::new("[^a-z0-9]+").map_err(|_| AppError::Internal)?;
                let normalized = pattern
                    .replace_all(&text.to_lowercase(), "-")
                    .trim_matches('-')
                    .to_owned();
                validation::slug(&normalized).map_err(invalid)?;
                json!(normalized)
            } else {
                if field == Field::Category
                    && input.content_type == ContentType::Blog
                    && !matches!(text, "insights" | "growth" | "ai" | "news")
                {
                    return Err(AppError::AiInvalidOutput);
                }
                json!(text)
            }
        }
    };
    Ok(value)
}

pub fn factual_gap(
    field: Field,
    value: &Value,
    input: &GenerateRequest,
) -> AppResult<Option<&'static str>> {
    if matches!(field, Field::CoverAlt | Field::ImageAlt | Field::OgImageAlt) {
        let description = match &input.content_context {
            ContentContext::Blog(context) => &context.image_description,
            ContentContext::Project(context)
            | ContentContext::CaseStudy(context)
            | ContentContext::Service(context) => &context.image_description,
        };
        if description.trim().is_empty() {
            return Ok(Some("No trustworthy image description was supplied"));
        }
    }
    if field == Field::Category && input.content_type == ContentType::Project {
        let supplied = match &input.content_context {
            ContentContext::Project(context) => context.project_type.as_str(),
            _ => "",
        };
        let existing = input
            .existing_fields
            .get(field.key())
            .and_then(Value::as_str)
            .unwrap_or_default();
        if value.as_str() != Some(supplied) && value.as_str() != Some(existing) {
            return Ok(Some("Project category was not supplied"));
        }
    }
    let text = value.to_string().to_lowercase();
    let source = facts(input);
    let urls = regex::Regex::new(r#"https?://[^\s\"<>\\]+"#).map_err(|_| AppError::Internal)?;
    if urls
        .find_iter(&text)
        .any(|url| !source.contains(url.as_str()))
    {
        return Ok(Some("Generated content included an unsupported URL"));
    }
    let metrics = regex::Regex::new(r"(?i)\b\d+(?:[.,]\d+)?\s*(?:%|milliseconds?\b|ms\b|seconds?\b|users?\b|customers?\b|visitors?\b|requests?\b|x faster\b)").map_err(|_| AppError::Internal)?;
    if metrics
        .find_iter(&text)
        .any(|metric| !source.contains(metric.as_str()))
    {
        return Ok(Some("An unsupported numeric claim was omitted"));
    }
    if matches!(
        input.content_type,
        ContentType::Project | ContentType::CaseStudy
    ) {
        let years = regex::Regex::new(r"\b20\d{2}\b").map_err(|_| AppError::Internal)?;
        if years
            .find_iter(&text)
            .any(|year| !source.contains(year.as_str()))
        {
            return Ok(Some("An unsupported date was omitted"));
        }
    }
    Ok(None)
}

pub fn derived(field: Field, input: &GenerateRequest, origin: &str) -> Option<Value> {
    if field == Field::Year {
        if let ContentContext::Project(context) = &input.content_context {
            if !context.year.is_empty() {
                return Some(json!(context.year));
            }
        }
        return None;
    }
    if !empty(input.existing_fields.get(field.key())) {
        return None;
    }
    if field == Field::CanonicalUrl {
        let slug = input.existing_fields.get("slug")?.as_str()?;
        if validation::slug(slug).is_err() {
            return None;
        }
        return Some(json!(format!(
            "{}/{}/{}",
            origin,
            match input.content_type {
                ContentType::Blog => "blog",
                ContentType::Project => "projects",
                ContentType::CaseStudy => "case-studies",
                ContentType::Service => "services",
            },
            slug
        )));
    }
    let names = if input.content_type == ContentType::Blog {
        ["fields.socialImage", "fields.cover"]
    } else {
        ["fields.image", "fields.image"]
    };
    for name in names {
        if let Some(url) = input.existing_fields.get(name).and_then(Value::as_str) {
            if validation::url(url, name, true, false).is_ok() {
                return Some(json!(if url.starts_with('/') {
                    format!("{origin}{url}")
                } else {
                    url.to_owned()
                }));
            }
        }
    }
    None
}

pub fn output(
    input: &GenerateRequest,
    draft: ProviderDraft,
    origin: &str,
) -> AppResult<GenerateResponse> {
    if draft
        .generated_fields
        .keys()
        .any(|field| !input.target_fields.contains(field) || field.derived())
        || draft.warnings.len() > 10
    {
        return Err(AppError::AiInvalidOutput);
    }
    let mut generated_fields = BTreeMap::new();
    let mut skipped_fields = Vec::new();
    let mut warnings = vec!["Review factual claims and generated content before saving. Existing inline images are retained when applying a new body.".into()];
    for warning in draft.warnings {
        validation::text(&warning, "warning", 1, 500).map_err(|_| AppError::AiInvalidOutput)?;
        warnings.push(warning);
    }
    for field in &input.target_fields {
        if input.mode == Mode::FillMissing && !empty(input.existing_fields.get(field.key())) {
            skipped_fields.push(SkippedField {
                field: *field,
                reason: "Existing value is populated".into(),
            });
            continue;
        }
        if *field == Field::CanonicalUrl
            && draft
                .generated_fields
                .get(&Field::Slug)
                .and_then(Value::as_str)
                .is_some_and(|slug| {
                    input.existing_fields.get("slug").and_then(Value::as_str) != Some(slug)
                })
        {
            skipped_fields.push(SkippedField {
                field: *field,
                reason:
                    "Apply the final slug first, then regenerate SEO to derive its canonical URL"
                        .into(),
            });
            continue;
        }
        let value = if field.derived() {
            derived(*field, input, origin)
        } else {
            draft
                .generated_fields
                .get(field)
                .filter(|value| !value.is_null())
                .cloned()
        };
        let Some(value) = value else {
            skipped_fields.push(SkippedField {
                field: *field,
                reason: "Insufficient facts or an existing asset/URL must be preserved".into(),
            });
            continue;
        };
        let value = if field.derived() {
            value
        } else {
            normalize(*field, value, input)?
        };
        if let Some(reason) = if field.derived() {
            None
        } else {
            factual_gap(*field, &value, input)?
        } {
            skipped_fields.push(SkippedField {
                field: *field,
                reason: reason.into(),
            });
        } else if empty(Some(&value)) {
            skipped_fields.push(SkippedField {
                field: *field,
                reason: "No new supported content was generated".into(),
            });
        } else {
            generated_fields.insert(*field, value);
        }
    }
    let response = GenerateResponse {
        usage: None,
        content_type: input.content_type,
        generation_id: uuid::Uuid::new_v4(),
        generated_fields,
        warnings,
        skipped_fields,
    };
    if serde_json::to_vec(&response)
        .map_err(|_| AppError::Internal)?
        .len()
        > 256_000
    {
        return Err(AppError::AiInvalidOutput);
    }
    Ok(response)
}
