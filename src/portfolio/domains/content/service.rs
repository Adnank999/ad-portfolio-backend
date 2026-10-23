use super::{dto::*, editor};
use crate::common::{
    error::{AppError, AppResult},
    validation::{self, escape},
};
use serde_json::Value;
use std::collections::HashSet;

pub fn prepare(kind: Kind, input: &mut ContentWrite) -> AppResult<(String, String)> {
    if kind == Kind::Service && input.editor_content.is_some() {
        return Err(AppError::Invalid(
            "Services use structured page sections rather than editorContent".into(),
        ));
    }
    validation::slug(&input.slug)?;
    validation::text(&input.title, "title", 1, 200)?;
    validation::text(&input.description, "description", 1, 1000)?;
    validate_seo(&input.seo)?;
    if input.faqs.len() > 30 {
        return Err(AppError::Invalid("At most 30 FAQs are supported".into()));
    }
    for faq in &input.faqs {
        validation::text(&faq.question, "FAQ question", 1, 300)?;
        validation::text(&faq.answer, "FAQ answer", 1, 5000)?;
    }
    let legacy = match kind {
        Kind::Blog => {
            let fields: BlogFields = serde_json::from_value(input.fields.clone())
                .map_err(|_| AppError::Invalid("fields must match the blog schema".into()))?;
            validate_blog(&fields)?;
            input.fields = serde_json::to_value(&fields).map_err(|_| AppError::Internal)?;
            render_blog(&fields)
        }
        Kind::Project => {
            let fields: ProjectFields = serde_json::from_value(input.fields.clone())
                .map_err(|_| AppError::Invalid("fields must match the project schema".into()))?;
            validate_project(&fields)?;
            input.fields = serde_json::to_value(&fields).map_err(|_| AppError::Internal)?;
            render_project(&fields)
        }
        Kind::CaseStudy => {
            let fields: CaseStudyFields = serde_json::from_value(input.fields.clone())
                .map_err(|_| AppError::Invalid("fields must match the case study schema".into()))?;
            validate_case_study(&fields)?;
            input.fields = serde_json::to_value(&fields).map_err(|_| AppError::Internal)?;
            render_case_study(&fields)
        }
        Kind::Service => {
            let fields: super::services::ServiceFields =
                serde_json::from_value(input.fields.clone()).map_err(|_| {
                    AppError::Invalid("fields must match the service schema".into())
                })?;
            super::services::validate(&fields)?;
            input.fields = serde_json::to_value(&fields).map_err(|_| AppError::Internal)?;
            super::services::render(&fields)
        }
    };
    let mut html = match &input.editor_content {
        Some(document) => editor::render_document(document)?,
        None => ammonia::Builder::default()
            .link_rel(Some("noopener noreferrer"))
            .clean(&legacy)
            .to_string(),
    };
    if !input.faqs.is_empty() {
        html.push_str("<section><h2>Frequently asked questions</h2>");
        for faq in &input.faqs {
            html.push_str(&format!(
                "<details><summary>{}</summary><p>{}</p></details>",
                escape(&faq.question),
                escape(&faq.answer)
            ));
        }
        html.push_str("</section>");
    }
    let plain = editor::plain_text(&html)?;
    if matches!(input.status, Status::Published) && plain.is_empty() {
        return Err(AppError::Invalid(
            "Published content needs a readable body".into(),
        ));
    }
    if html.len() > 1_500_000 {
        return Err(AppError::Invalid("Rendered document is too large".into()));
    }
    Ok((html, plain))
}

fn validate_seo(seo: &Seo) -> AppResult<()> {
    if let Some(value) = &seo.structured_data {
        validate_structured_data(value)?;
    }
    for value in [&seo.meta_title, &seo.og_title, &seo.twitter_title]
        .into_iter()
        .flatten()
    {
        validation::text(value, "SEO title", 1, 200)?;
    }
    for value in [
        &seo.meta_description,
        &seo.og_description,
        &seo.twitter_description,
    ]
    .into_iter()
    .flatten()
    {
        validation::text(value, "SEO description", 1, 1000)?;
    }
    if let Some(value) = &seo.canonical_url {
        validation::url(value, "canonicalUrl", true, false)?;
    }
    for value in [&seo.og_image, &seo.twitter_image].into_iter().flatten() {
        validation::url(value, "SEO image", true, false)?;
    }
    if let Some(value) = &seo.og_image_alt {
        validation::text(value, "ogImageAlt", 0, 500)?;
    }
    Ok(())
}

fn strings(values: &[String], field: &str, maximum: usize) -> AppResult<()> {
    if values.len() > maximum {
        return Err(AppError::Invalid(format!("Too many {field} entries")));
    }
    for value in values {
        validation::text(value, field, 1, 20_000)?;
    }
    Ok(())
}

fn validate_blog(fields: &BlogFields) -> AppResult<()> {
    if !matches!(
        fields.category.as_str(),
        "insights" | "growth" | "ai" | "news"
    ) {
        return Err(AppError::Invalid("Unsupported blog category".into()));
    }
    validation::url(&fields.cover, "cover", true, false)?;
    validation::url(&fields.social_image, "socialImage", true, false)?;
    validation::text(&fields.cover_alt, "coverAlt", 1, 500)?;
    strings(&fields.intro, "intro", 30)?;
    strings(&fields.takeaways, "takeaways", 30)?;
    strings(&fields.tags, "tags", 30)?;
    if fields.sections.len() > 60 {
        return Err(AppError::Invalid(
            "At most 60 sections are supported".into(),
        ));
    }
    let mut ids = HashSet::new();
    for section in &fields.sections {
        validation::slug(&section.id)?;
        if !ids.insert(&section.id) {
            return Err(AppError::Invalid("Section IDs must be unique".into()));
        }
        validation::text(&section.title, "section title", 1, 300)?;
        strings(&section.paragraphs, "paragraphs", 40)?;
        if let Some(points) = &section.points {
            strings(points, "points", 50)?;
        }
        if let Some(cards) = &section.cards {
            if cards.len() > 12 {
                return Err(AppError::Invalid("At most 12 cards per section".into()));
            }
            for card in cards {
                validation::text(&card.title, "card title", 1, 300)?;
                validation::text(&card.text, "card text", 1, 5000)?;
            }
        }
        if let Some(card) = &section.callout {
            validation::text(&card.title, "callout title", 1, 300)?;
            validation::text(&card.text, "callout text", 1, 10_000)?;
        }
        if let Some(table) = &section.table {
            validation::text(&table.caption, "table caption", 1, 300)?;
            strings(&table.headers, "table headers", 10)?;
            if table.headers.is_empty()
                || table.rows.len() > 100
                || table
                    .rows
                    .iter()
                    .any(|row| row.len() != table.headers.len())
            {
                return Err(AppError::Invalid(
                    "Table rows must match its 1 to 10 headers (maximum 100 rows)".into(),
                ));
            }
            for row in &table.rows {
                strings(row, "table cells", 10)?;
            }
        }
    }
    Ok(())
}

fn validate_images(image: &str, image_alt: &str, gallery: &[ProjectImage]) -> AppResult<()> {
    validation::url(image, "image", true, false)?;
    validation::text(image_alt, "imageAlt", 1, 500)?;
    if gallery.len() > 20 {
        return Err(AppError::Invalid(
            "At most 20 project gallery images are supported".into(),
        ));
    }
    let mut gallery_urls = HashSet::new();
    for image in gallery {
        validation::url(&image.url, "gallery image URL", true, false)?;
        validation::text(&image.alt, "gallery image alt text", 1, 500)?;
        validation::text(&image.caption, "gallery image caption", 0, 1000)?;
        if !gallery_urls.insert(&image.url) {
            return Err(AppError::Invalid(
                "Project gallery image URLs must be unique".into(),
            ));
        }
    }
    Ok(())
}

fn validate_project(fields: &ProjectFields) -> AppResult<()> {
    validate_images(&fields.image, &fields.image_alt, &fields.gallery)?;
    validation::text(&fields.category, "category", 1, 100)?;
    if fields.year.len() != 4 || !fields.year.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::Invalid("year must be four digits".into()));
    }
    validation::url(&fields.github_url, "githubUrl", false, true)?;
    validation::url(&fields.live_url, "liveUrl", false, true)?;
    strings(&fields.technologies, "technologies", 40)?;
    strings(&fields.features, "features", 100)?;
    for value in [
        &fields.overview,
        &fields.problem,
        &fields.solution,
        &fields.technical,
        &fields.challenges,
        &fields.results,
    ] {
        validation::text(value, "project narrative", 0, 50_000)?;
    }
    Ok(())
}

fn paragraphs(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("<p>{}</p>", escape(value)))
        .collect()
}

pub const INDUSTRIES: &[&str] = &[
    "manufacturing",
    "retail-ecommerce",
    "healthcare",
    "education",
    "hospitality-real-estate",
    "media-publishing",
    "music-entertainment",
    "technology-saas",
    "logistics",
    "non-profits",
];

fn validate_case_study(fields: &CaseStudyFields) -> AppResult<()> {
    if !INDUSTRIES.contains(&fields.industry.as_str())
        || !matches!(fields.study_type.as_str(), "portfolio" | "concept")
    {
        return Err(AppError::Invalid(
            "Choose a supported industry and study type".into(),
        ));
    }
    validate_images(&fields.image, &fields.image_alt, &fields.gallery)?;
    strings(&fields.technologies, "technologies", 40)?;
    for value in [&fields.context, &fields.problem, &fields.solution] {
        validation::text(value, "case study narrative", 0, 50_000)?;
    }
    strings(&fields.lessons, "lessons", 50)?;
    for items in [&fields.steps, &fields.outcomes] {
        if items.len() > 30 {
            return Err(AppError::Invalid(
                "At most 30 steps or outcomes are supported".into(),
            ));
        }
        for item in items {
            validation::text(&item.title, "item title", 1, 300)?;
            validation::text(&item.text, "item text", 1, 20_000)?;
        }
    }
    if !fields.project_slug.is_empty() {
        validation::slug(&fields.project_slug)?;
    }
    Ok(())
}

fn validate_structured_data(value: &Value) -> AppResult<()> {
    let valid_root = value.as_object().is_some_and(|object| {
        object.get("@context").and_then(Value::as_str) == Some("https://schema.org")
            && (object.get("@type").is_some_and(Value::is_string)
                || object.get("@graph").is_some_and(|graph| {
                    graph.as_array().is_some_and(|nodes| {
                        !nodes.is_empty() && nodes.len() <= 30 && nodes.iter().all(Value::is_object)
                    })
                }))
    });
    fn bounded(value: &Value, depth: usize) -> bool {
        depth <= 16
            && match value {
                Value::Array(values) => {
                    values.len() <= 100 && values.iter().all(|value| bounded(value, depth + 1))
                }
                Value::Object(values) => {
                    values.len() <= 100 && values.values().all(|value| bounded(value, depth + 1))
                }
                _ => true,
            }
    }
    if !valid_root || value.to_string().len() > 64_000 || !bounded(value, 0) {
        return Err(AppError::Invalid(
            "JSON-LD must be a bounded schema.org object with @type or @graph".into(),
        ));
    }
    Ok(())
}

fn render_case_study(fields: &CaseStudyFields) -> String {
    let mut html = String::new();
    for (heading, text) in [
        ("Context", &fields.context),
        ("Problem", &fields.problem),
        ("Solution", &fields.solution),
    ] {
        html.push_str(&format!("<h2>{heading}</h2><p>{}</p>", escape(text)));
    }
    for (heading, items) in [("Approach", &fields.steps), ("Outcomes", &fields.outcomes)] {
        html.push_str(&format!("<h2>{heading}</h2>"));
        for item in items {
            html.push_str(&format!(
                "<h3>{}</h3><p>{}</p>",
                escape(&item.title),
                escape(&item.text)
            ));
        }
    }
    html.push_str("<h2>Technologies</h2>");
    html.push_str(&list(&fields.technologies));
    html.push_str("<h2>Lessons</h2>");
    html.push_str(&list(&fields.lessons));
    html
}
fn list(values: &[String]) -> String {
    format!(
        "<ul>{}</ul>",
        values
            .iter()
            .map(|value| format!("<li>{}</li>", escape(value)))
            .collect::<String>()
    )
}

fn render_blog(fields: &BlogFields) -> String {
    let mut html = paragraphs(&fields.intro);
    if !fields.takeaways.is_empty() {
        html.push_str("<h2>Key takeaways</h2>");
        html.push_str(&list(&fields.takeaways));
    }
    for section in &fields.sections {
        html.push_str(&format!(
            "<section><h2 id=\"{}\">{}</h2>{}",
            escape(&section.id),
            escape(&section.title),
            paragraphs(&section.paragraphs)
        ));
        if let Some(points) = &section.points {
            html.push_str(&list(points));
        }
        if let Some(cards) = &section.cards {
            for card in cards {
                html.push_str(&format!(
                    "<h3>{}</h3><p>{}</p>",
                    escape(&card.title),
                    escape(&card.text)
                ));
            }
        }
        if let Some(card) = &section.callout {
            html.push_str(&format!(
                "<aside><h3>{}</h3><p>{}</p></aside>",
                escape(&card.title),
                escape(&card.text)
            ));
        }
        if let Some(table) = &section.table {
            html.push_str(&format!(
                "<table><caption>{}</caption><thead><tr>",
                escape(&table.caption)
            ));
            for header in &table.headers {
                html.push_str(&format!("<th scope=\"col\">{}</th>", escape(header)));
            }
            html.push_str("</tr></thead><tbody>");
            for row in &table.rows {
                html.push_str("<tr>");
                for cell in row {
                    html.push_str(&format!("<td>{}</td>", escape(cell)));
                }
                html.push_str("</tr>");
            }
            html.push_str("</tbody></table>");
        }
        html.push_str("</section>");
    }
    html
}

fn render_project(fields: &ProjectFields) -> String {
    let mut html = String::new();
    for (heading, value) in [
        ("Overview", &fields.overview),
        ("The problem", &fields.problem),
        ("The solution", &fields.solution),
        ("Technical approach", &fields.technical),
        ("Challenges", &fields.challenges),
        ("Results", &fields.results),
    ] {
        if !value.is_empty() {
            html.push_str(&format!("<h2>{heading}</h2><p>{}</p>", escape(value)));
        }
    }
    if !fields.features.is_empty() {
        html.push_str("<h2>Features</h2>");
        html.push_str(&list(&fields.features));
    }
    html
}

pub fn json<T: serde::Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|_| AppError::Internal)
}
