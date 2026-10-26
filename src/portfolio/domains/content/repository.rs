use super::{dto::*, seo, service};
use crate::{
    common::{
        error::{AppError, AppResult},
        state::AppState,
        validation,
    },
    domains::auth::Principal,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

pub(super) const SELECT: &str = "SELECT c.*,u.name AS author_name,u.headline AS author_headline,ARRAY(SELECT asset_id FROM content_media_assets WHERE content_id=c.id) AS media_asset_ids FROM content_items c JOIN users u ON u.id=c.author_id ";

#[derive(Clone, FromRow)]
pub struct Record {
    pub media_asset_ids: Vec<Uuid>,
    pub id: Uuid,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub fields: Value,
    pub editor_content: Option<Value>,
    pub rendered_html: String,
    pub plain_text: String,
    pub seo: Value,
    pub faqs: Value,
    pub author_id: Uuid,
    pub author_name: String,
    pub author_headline: String,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

fn filtered(
    base: &str,
    kind: Kind,
    query: &ListQuery,
    admin: bool,
) -> QueryBuilder<'static, Postgres> {
    let mut builder = QueryBuilder::new(base.to_owned());
    builder
        .push(" WHERE c.kind=")
        .push_bind(kind.as_str())
        .push(" AND c.deleted_at IS NULL");
    if !admin {
        builder.push(" AND c.status='published' AND c.published_at<=now()");
        if kind == Kind::Service {
            builder.push(" AND c.fields->>'enabled'='true'");
        }
    }
    if let Some(status) = &query.status {
        builder.push(" AND c.status=").push_bind(status.clone());
    }
    if let Some(category) = &query.category {
        builder
            .push(if kind == Kind::CaseStudy {
                " AND c.fields->>'industry'="
            } else {
                " AND c.fields->>'category'="
            })
            .push_bind(category.clone());
    }
    if let Some(tag) = &query.tag {
        builder
            .push(" AND COALESCE(c.fields->'tags','[]'::jsonb) ? ")
            .push_bind(tag.clone());
    }
    if let Some(featured) = query.featured {
        builder
            .push(" AND COALESCE((c.fields->>'featured')::boolean,false)=")
            .push_bind(featured);
    }
    if let Some(search) = &query.q {
        builder
            .push(" AND c.search_document @@ websearch_to_tsquery('english',")
            .push_bind(search.clone())
            .push(")");
    }
    builder
}

pub async fn list(state: &AppState, kind: Kind, query: ListQuery, admin: bool) -> AppResult<Value> {
    let page = query.page.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(20);
    if !(1..=1_000_000).contains(&page) || !(1..=100).contains(&per_page) {
        return Err(AppError::Invalid(
            "page must be 1 to 1000000 and perPage 1 to 100".into(),
        ));
    }
    if let Some(status) = &query.status {
        if !admin || !matches!(status.as_str(), "draft" | "published" | "archived") {
            return Err(AppError::Invalid(
                "status is an admin-only filter: draft, published, or archived".into(),
            ));
        }
    }
    for (name, value) in [
        ("category", &query.category),
        ("tag", &query.tag),
        ("q", &query.q),
    ] {
        if let Some(value) = value {
            validation::text(value, name, 1, 120)?;
        }
    }
    let order = match query.sort.as_deref().unwrap_or("newest") {
        "newest" => " ORDER BY COALESCE(c.published_at,c.created_at) DESC,c.id DESC",
        "oldest" => " ORDER BY COALESCE(c.published_at,c.created_at) ASC,c.id ASC",
        "title" => " ORDER BY c.title ASC,c.id ASC",
        _ => {
            return Err(AppError::Invalid(
                "sort must be newest, oldest, or title".into(),
            ))
        }
    };
    let total: i64 = filtered("SELECT count(*) FROM content_items c", kind, &query, admin)
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await?;
    let mut builder = filtered(SELECT, kind, &query, admin);
    builder
        .push(order)
        .push(" LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1) * per_page);
    let rows = builder
        .build_query_as::<Record>()
        .fetch_all(&state.pool)
        .await?;
    let data = rows
        .iter()
        .map(|record| present(record, &state.config, admin, false))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(
        json!({"data":data,"pagination":{"page":page,"perPage":per_page,"total":total,"totalPages":(total+per_page-1)/per_page}}),
    )
}

pub async fn by_id(pool: &PgPool, kind: Kind, id: Uuid) -> AppResult<Record> {
    sqlx::query_as(&format!(
        "{SELECT} WHERE c.id=$1 AND c.kind=$2 AND c.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(kind.as_str())
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn by_slug(pool: &PgPool, kind: Kind, slug: &str) -> AppResult<Record> {
    validation::slug(slug).map_err(|_| AppError::NotFound)?;
    sqlx::query_as(&format!("{SELECT} JOIN content_slugs alias ON alias.content_id=c.id WHERE alias.kind=$1 AND alias.slug=$2 AND c.kind=$1 AND c.status='published' AND c.published_at<=now() AND c.deleted_at IS NULL AND (c.kind!='service' OR c.fields->>'enabled'='true')"))
        .bind(kind.as_str()).bind(slug).fetch_optional(pool).await?.ok_or(AppError::NotFound)
}

pub async fn save(
    state: &AppState,
    kind: Kind,
    id: Option<Uuid>,
    user: &Principal,
    mut input: ContentWrite,
) -> AppResult<Record> {
    user.require(&kind.permission("write"))?;
    let (html, plain) = service::prepare(kind, &mut input)?;
    let mut transaction = state.pool.begin().await?;
    let current = if let Some(id) = id {
        Some(
            sqlx::query_as::<_, Record>(&format!(
                "{SELECT} WHERE c.id=$1 AND c.kind=$2 AND c.deleted_at IS NULL FOR UPDATE OF c"
            ))
            .bind(id)
            .bind(kind.as_str())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::NotFound)?,
        )
    } else {
        None
    };
    if !matches!(input.status, Status::Draft)
        || current
            .as_ref()
            .is_some_and(|record| record.status == "published")
    {
        user.require(&kind.permission("publish"))?;
    }
    if let Some(record) = &current {
        if input.version != Some(record.version) {
            return Err(AppError::Conflict(
                "Content changed; reload it before saving".into(),
            ));
        }
    } else if input.version.is_some() {
        return Err(AppError::Invalid(
            "Do not provide version when creating content".into(),
        ));
    }
    let content_id = id.unwrap_or_else(Uuid::new_v4);
    if kind == Kind::Service {
        super::visuals::validate_assignments(
            &mut transaction,
            &input.fields,
            current.as_ref().map(|record| &record.fields),
        )
        .await?;
    }
    let previous_assets = current
        .as_ref()
        .map(|record| record.media_asset_ids.clone())
        .unwrap_or_default();
    let used_assets = crate::domains::media::service::validate_references(
        &mut transaction,
        &input,
        current.as_ref(),
        kind,
        user,
        state
            .config
            .r2
            .as_ref()
            .map(|config| config.public_base_url.as_str()),
    )
    .await?;
    let published_at = input
        .published_at
        .or_else(|| current.as_ref().and_then(|record| record.published_at))
        .or_else(|| matches!(input.status, Status::Published).then(Utc::now));
    if current.is_some() {
        let reserved=sqlx::query("INSERT INTO content_slugs(kind,slug,content_id) VALUES($1,$2,$3) ON CONFLICT(kind,slug) DO UPDATE SET content_id=excluded.content_id WHERE content_slugs.content_id=excluded.content_id").bind(kind.as_str()).bind(&input.slug).bind(content_id).execute(&mut *transaction).await?;
        if reserved.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "This slug is reserved by another content item".into(),
            ));
        }
        sqlx::query("UPDATE content_items SET slug=$1,title=$2,description=$3,status=$4,fields=$5,editor_content=$6,rendered_html=$7,plain_text=$8,seo=$9,faqs=$10,published_at=$11,updated_at=now(),version=version+1 WHERE id=$12")
            .bind(&input.slug).bind(input.title.trim()).bind(input.description.trim()).bind(input.status.as_str()).bind(input.fields).bind(input.editor_content).bind(html).bind(plain).bind(service::json(&input.seo)?).bind(service::json(&input.faqs)?).bind(published_at).bind(content_id).execute(&mut *transaction).await?;
    } else {
        sqlx::query("INSERT INTO content_items(id,kind,slug,title,description,status,fields,editor_content,rendered_html,plain_text,seo,faqs,author_id,published_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)")
            .bind(content_id).bind(kind.as_str()).bind(&input.slug).bind(input.title.trim()).bind(input.description.trim()).bind(input.status.as_str()).bind(input.fields).bind(input.editor_content).bind(html).bind(plain).bind(service::json(&input.seo)?).bind(service::json(&input.faqs)?).bind(user.id).bind(published_at).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO content_slugs(kind,slug,content_id) VALUES($1,$2,$3)")
            .bind(kind.as_str())
            .bind(input.slug)
            .bind(content_id)
            .execute(&mut *transaction)
            .await?;
    }
    crate::domains::media::service::save_references(
        &mut transaction,
        content_id,
        &previous_assets,
        &used_assets,
    )
    .await?;
    transaction.commit().await?;
    by_id(&state.pool, kind, content_id).await
}

pub async fn delete(state: &AppState, kind: Kind, id: Uuid, version: i32) -> AppResult<()> {
    let result=sqlx::query("UPDATE content_items SET deleted_at=now(),updated_at=now(),version=version+1 WHERE id=$1 AND kind=$2 AND deleted_at IS NULL AND version=$3").bind(id).bind(kind.as_str()).bind(version).execute(&state.pool).await?;
    if result.rows_affected() != 1 {
        by_id(&state.pool, kind, id).await?;
        return Err(AppError::Conflict(
            "Content changed; reload it before deleting".into(),
        ));
    }
    Ok(())
}

pub fn present(
    record: &Record,
    config: &crate::common::config::Config,
    admin: bool,
    detail: bool,
) -> AppResult<Value> {
    let mut value = if admin {
        json!({})
    } else {
        record.fields.clone()
    };
    let object = value.as_object_mut().ok_or(AppError::Internal)?;
    object.insert("id".into(), json!(record.id));
    object.insert("slug".into(), json!(record.slug));
    object.insert("title".into(), json!(record.title));
    object.insert("description".into(), json!(record.description));
    object.insert(
        "publishedAt".into(),
        json!(record
            .published_at
            .map(|date| date.format("%Y-%m-%d").to_string())),
    );
    object.insert(
        "updatedAt".into(),
        json!(record.updated_at.format("%Y-%m-%d").to_string()),
    );
    object.insert("publishedAtTimestamp".into(), json!(record.published_at));
    object.insert("updatedAtTimestamp".into(), json!(record.updated_at));
    let initials = record
        .author_name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect::<String>();
    object.insert("author".into(),json!({"id":record.author_id,"name":record.author_name,"role":record.author_headline,"initials":initials}));
    object.insert(
        "readingMinutes".into(),
        json!(record
            .plain_text
            .split_whitespace()
            .count()
            .div_ceil(220)
            .max(1)),
    );
    if admin {
        object.insert("assetIds".into(), json!(record.media_asset_ids));
        object.insert("publishedAt".into(), json!(record.published_at));
        object.insert("updatedAt".into(), json!(record.updated_at));
        object.insert("status".into(), json!(record.status));
        object.insert("version".into(), json!(record.version));
        object.insert("createdAt".into(), json!(record.created_at));
        object.insert("fields".into(), record.fields.clone());
        object.insert("seo".into(), record.seo.clone());
    }
    if detail {
        object.insert("editorContent".into(), json!(record.editor_content));
        object.insert("renderedHtml".into(), json!(record.rendered_html));
        object.insert("faqs".into(), record.faqs.clone());
        if !admin {
            object.insert("seo".into(), seo::resolved(record, config)?);
            object.insert(
                "structuredData".into(),
                seo::structured_data(record, config)?,
            );
        }
    } else {
        for field in [
            "intro",
            "takeaways",
            "sections",
            "overview",
            "problem",
            "solution",
            "features",
            "technical",
            "challenges",
            "results",
            "context",
            "steps",
            "outcomes",
            "lessons",
            "pageContent",
        ] {
            object.remove(field);
        }
        if admin {
            if record.kind == "service" {
                object.insert("enabled".into(), record.fields["enabled"].clone());
                object.insert("navEnabled".into(), record.fields["navEnabled"].clone());
            }
            object.remove("fields");
            object.insert(
                "category".into(),
                record
                    .fields
                    .get(if record.kind == "case_study" {
                        "industry"
                    } else {
                        "category"
                    })
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
    }
    Ok(value)
}
