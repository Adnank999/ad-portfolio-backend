use super::{
    dto::{Faq, Seo},
    repository::Record,
};
use crate::common::{
    config::Config,
    error::{AppError, AppResult},
    validation::escape,
};
use serde_json::{json, Value};

pub fn page_url(record: &Record, config: &Config) -> String {
    format!(
        "{}/{}/{}",
        config.frontend_url,
        match record.kind.as_str() {
            "blog" => "blog",
            "case_study" => "case-studies",
            "service" => "services",
            _ => "projects",
        },
        record.slug
    )
}
fn absolute(value: &str, config: &Config) -> String {
    if value.starts_with('/') {
        format!("{}{value}", config.frontend_url)
    } else {
        value.to_owned()
    }
}

pub fn resolved(record: &Record, config: &Config) -> AppResult<Value> {
    let seo: Seo = serde_json::from_value(record.seo.clone()).map_err(|_| AppError::Internal)?;
    let hero_visual = record
        .fields
        .get("visuals")
        .and_then(|visuals| visuals.get("hero"));
    let service_image = record
        .fields
        .get("media")
        .and_then(Value::as_array)
        .and_then(|media| media.iter().find(|media| media["slot"] == "hero"))
        .and_then(|media| {
            media.get(if media["kind"] == "video" {
                "poster"
            } else {
                "url"
            })
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let fallback_image = hero_visual
        .and_then(|visual| {
            visual.get(if visual["type"] == "shader" {
                "poster"
            } else {
                "src"
            })
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or(service_image)
        .or_else(|| {
            (record.kind == "project")
                .then(|| record.fields["featuredImageUrl"].as_str())
                .flatten()
                .filter(|url| !url.is_empty())
        })
        .or_else(|| {
            record
                .fields
                .get(if record.kind == "blog" {
                    "socialImage"
                } else {
                    "image"
                })
                .and_then(Value::as_str)
        })
        .unwrap_or("/images/case-studies-cover.png");
    let image = absolute(seo.og_image.as_deref().unwrap_or(fallback_image), config);
    let alt = seo
        .og_image_alt
        .as_deref()
        .or_else(|| {
            if record.kind == "project" {
                let featured = record.fields["featuredImageUrl"].as_str()?;
                return record.fields["gallery"]
                    .as_array()?
                    .iter()
                    .find(|image| image["url"].as_str() == Some(featured))?
                    .get("alt")?
                    .as_str();
            }
            None
        })
        .or_else(|| hero_visual.and_then(|visual| visual["alt"].as_str()))
        .or_else(|| {
            record
                .fields
                .get("media")
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find(|item| item["slot"] == "hero"))
                .and_then(|item| item["alt"].as_str())
        })
        .or_else(|| {
            record
                .fields
                .get(if record.kind == "blog" {
                    "coverAlt"
                } else {
                    "imageAlt"
                })
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    let no_index = seo.no_index
        || (record.kind == "service" && record.fields["enabled"] != true)
        || record.status != "published"
        || record
            .published_at
            .is_none_or(|date| date > chrono::Utc::now());
    Ok(
        json!({"metaTitle":seo.meta_title.as_deref().unwrap_or(&record.title),"metaDescription":seo.meta_description.as_deref().unwrap_or(&record.description),"canonicalUrl":seo.canonical_url.as_deref().map(|value|absolute(value,config)).unwrap_or_else(||page_url(record,config)),"ogTitle":seo.og_title.as_deref().or(seo.meta_title.as_deref()).unwrap_or(&record.title),"ogDescription":seo.og_description.as_deref().or(seo.meta_description.as_deref()).unwrap_or(&record.description),"ogImage":image,"ogImageAlt":alt,"ogType":if record.kind=="blog" {"article"} else {"website"},"twitterCard":"summary_large_image","twitterTitle":seo.twitter_title.as_deref().or(seo.og_title.as_deref()).or(seo.meta_title.as_deref()).unwrap_or(&record.title),"twitterDescription":seo.twitter_description.as_deref().or(seo.og_description.as_deref()).or(seo.meta_description.as_deref()).unwrap_or(&record.description),"twitterImage":seo.twitter_image.as_deref().map(|value|absolute(value,config)).unwrap_or_else(||image.clone()),"noIndex":no_index,"noFollow":seo.no_follow,"robots":format!("{},{}",if no_index {"noindex"} else {"index"},if seo.no_follow {"nofollow"} else {"follow"})}),
    )
}

pub fn structured_data(record: &Record, config: &Config) -> AppResult<Value> {
    let metadata = resolved(record, config)?;
    let url = metadata["canonicalUrl"]
        .as_str()
        .ok_or(AppError::Internal)?;
    let (collection, path) = match record.kind.as_str() {
        "blog" => ("Blog", "blog"),
        "case_study" => ("Case studies", "case-studies"),
        "service" => ("Services", "services"),
        _ => ("Work", "projects"),
    };
    let mut graph = vec![
        json!({"@type":if record.kind=="blog" {"BlogPosting"} else if record.kind=="service" {"Service"} else {"CreativeWork"},"@id":format!("{url}#content"),"url":url,"headline":record.title,"name":record.title,"description":record.description,"image":metadata["ogImage"],"datePublished":record.published_at,"dateModified":record.updated_at,"inLanguage":"en","author":{"@type":"Person","name":record.author_name},"mainEntityOfPage":{"@type":"WebPage","@id":url}}),
        json!({"@type":"BreadcrumbList","itemListElement":[{"@type":"ListItem","position":1,"name":"Home","item":config.frontend_url},{"@type":"ListItem","position":2,"name":collection,"item":format!("{}/{path}",config.frontend_url)},{"@type":"ListItem","position":3,"name":record.title,"item":url}]}),
    ];
    if record.kind == "service" {
        graph[0]["provider"] = json!({"@type":"Person","name":record.author_name});
        graph[0]["serviceType"] = record.fields["category"].clone();
    }
    let faqs: Vec<Faq> =
        serde_json::from_value(record.faqs.clone()).map_err(|_| AppError::Internal)?;
    let seo: Seo = serde_json::from_value(record.seo.clone()).map_err(|_| AppError::Internal)?;
    if let Some(mut custom) = seo.structured_data {
        if let Some(nodes) = custom.get_mut("@graph").and_then(Value::as_array_mut) {
            graph.append(nodes);
        } else if let Some(object) = custom.as_object_mut() {
            object.remove("@context");
            graph.push(custom);
        }
    }
    if !faqs.is_empty() {
        graph.push(json!({"@type":"FAQPage","@id":format!("{url}#faq"),"mainEntity":faqs.iter().map(|faq|json!({"@type":"Question","name":faq.question,"acceptedAnswer":{"@type":"Answer","text":faq.answer}})).collect::<Vec<_>>()}));
    }
    Ok(json!({"@context":"https://schema.org","@graph":graph}))
}

pub async fn sitemap(pool: &sqlx::PgPool, config: &Config) -> AppResult<String> {
    let rows=sqlx::query_as::<_,Record>(&format!("{} WHERE c.status='published' AND c.published_at<=now() AND c.deleted_at IS NULL AND COALESCE((c.seo->>'noIndex')::boolean,false)=false ORDER BY c.kind,c.slug", super::repository::SELECT)).fetch_all(pool).await?;
    let mut xml=String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">");
    for record in rows {
        if record.kind == "service" && record.fields["enabled"] != true {
            continue;
        }
        let canonical = resolved(&record, config)?;
        let url = page_url(&record, config);
        if canonical["canonicalUrl"].as_str() != Some(&url) {
            continue;
        }
        xml.push_str(&format!(
            "<url><loc>{}</loc><lastmod>{}</lastmod></url>",
            escape(&url),
            record.updated_at.to_rfc3339()
        ));
    }
    xml.push_str("</urlset>");
    Ok(xml)
}
