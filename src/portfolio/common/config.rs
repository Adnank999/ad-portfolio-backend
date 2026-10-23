use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{env, time::Duration};
use url::Url;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub frontend_url: String,
    pub api_url: String,
    pub cors_origins: Vec<String>,
    pub max_connections: u32,
    pub access_minutes: i64,
    pub refresh_days: i64,
    pub reset_email: Option<ResetEmailConfig>,
    pub ai: Option<crate::domains::ai::config::AiConfig>,
}

#[derive(Clone)]
pub struct ResetEmailConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Ok(path) = env::var("PORTFOLIO_ENV_FILE") {
            dotenvy::from_path(path)?;
        } else if std::path::Path::new(".env.portfolio").exists() {
            dotenvy::from_filename(".env.portfolio")?;
        }
        let frontend_url = site_origin(
            &env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
        )?;
        let api_url = site_origin(
            &env::var("API_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:8080".into()),
        )?;
        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| frontend_url.clone())
            .split(',')
            .map(|value| site_origin(value.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        let config = Self {
            database_url: env::var("DATABASE_URL")?,
            host: env::var("SERVICE_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: env::var("SERVICE_PORT")
                .unwrap_or_else(|_| "8080".into())
                .parse()?,
            frontend_url,
            api_url,
            cors_origins,
            max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "10".into())
                .parse()?,
            access_minutes: env::var("ACCESS_TOKEN_MINUTES")
                .unwrap_or_else(|_| "30".into())
                .parse()?,
            refresh_days: env::var("REFRESH_TOKEN_DAYS")
                .unwrap_or_else(|_| "7".into())
                .parse()?,
            reset_email: match env::var("SMTP_HOST") {
                Ok(host) => Some(ResetEmailConfig {
                    host,
                    port: env::var("SMTP_PORT")
                        .unwrap_or_else(|_| "587".into())
                        .parse()?,
                    username: env::var("SMTP_USERNAME")?,
                    password: env::var("SMTP_PASSWORD")?,
                    from: env::var("SMTP_FROM")?,
                }),
                Err(_) => None,
            },
            ai: crate::domains::ai::config::AiConfig::from_env()?,
        };
        if !(1..=100).contains(&config.max_connections)
            || !(1..=120).contains(&config.access_minutes)
            || !(1..=30).contains(&config.refresh_days)
        {
            return Err("Database or session configuration is outside permitted bounds".into());
        }
        Ok(config)
    }

    pub async fn connect(&self) -> Result<PgPool, sqlx::Error> {
        PgPoolOptions::new()
            .max_connections(self.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&self.database_url)
            .await
    }
}

fn site_origin(value: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let url = Url::parse(value)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Site URLs and CORS origins must be HTTP(S) origins without paths or credentials"
                .into(),
        );
    }
    Ok(url.origin().ascii_serialization())
}
