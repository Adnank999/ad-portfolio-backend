use super::{
    dto::{ContentWrite, Kind, Status},
    service,
};
use crate::common::error::{AppError, AppResult};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn import_case_studies(pool: &PgPool) -> AppResult<u64> {
    import_records(
        pool,
        Kind::CaseStudy,
        include_str!("../../../../examples/case-studies.json"),
    )
    .await
}

pub async fn import_services(pool: &PgPool) -> AppResult<u64> {
    import_records(
        pool,
        Kind::Service,
        include_str!("../../../../examples/services.json"),
    )
    .await
}

async fn import_records(pool: &PgPool, kind: Kind, source: &str) -> AppResult<u64> {
    let mut inputs: Vec<ContentWrite> = serde_json::from_str(source)
        .map_err(|_| AppError::Invalid("Invalid content import data".into()))?;
    let mut prepared = Vec::new();
    for input in &mut inputs {
        input.status = Status::Published;
        prepared.push(service::prepare(kind, input)?);
    }
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("portfolio-{}-import", kind.as_str()))
        .execute(&mut *transaction)
        .await?;
    let author: Option<Uuid> = sqlx::query_scalar("SELECT u.id FROM users u JOIN user_roles ur ON ur.user_id=u.id JOIN roles r ON r.id=ur.role_id WHERE r.name='owner' AND u.active=true ORDER BY u.created_at,u.id LIMIT 1").fetch_optional(&mut *transaction).await?;
    let author = author
        .ok_or_else(|| AppError::Invalid("Bootstrap an owner before importing content".into()))?;
    let mut count = 0;
    for (input, (html, plain)) in inputs.into_iter().zip(prepared) {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM content_slugs WHERE kind=$2 AND slug=$1)",
        )
        .bind(&input.slug)
        .bind(kind.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if exists {
            continue;
        }
        let id = Uuid::new_v4();
        let result = sqlx::query("INSERT INTO content_items(id,kind,slug,title,description,status,fields,rendered_html,plain_text,seo,faqs,author_id,published_at) VALUES($1,$11,$2,$3,$4,'published',$5,$6,$7,$8,$9,$10,now()) ON CONFLICT(kind,slug) DO NOTHING")
            .bind(id).bind(&input.slug).bind(input.title).bind(input.description).bind(input.fields)
            .bind(html).bind(plain).bind(serde_json::to_value(input.seo).map_err(|_| AppError::Internal)?)
            .bind(service::json(&input.faqs)?).bind(author).bind(kind.as_str())
            .execute(&mut *transaction).await?;
        if result.rows_affected() == 1 {
            sqlx::query("INSERT INTO content_slugs(kind,slug,content_id) VALUES($3,$1,$2)")
                .bind(input.slug)
                .bind(id)
                .bind(kind.as_str())
                .execute(&mut *transaction)
                .await?;
            count += 1;
        }
    }
    transaction.commit().await?;
    Ok(count)
}
