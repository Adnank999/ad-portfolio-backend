use crate::common::{
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
    validation,
};
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone)]
pub struct ContactConfig {
    pub api_key: String,
    pub from: String,
    pub to: String,
}

impl ContactConfig {
    pub fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let api_key = env::var("RESEND_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Ok(None);
        }
        let config = Self {
            api_key,
            from: env::var("RESEND_FROM_EMAIL")?,
            to: env::var("CONTACT_TO_EMAIL")?,
        };
        config
            .validate()
            .map_err(|_| "Invalid Resend contact configuration")?;
        Ok(Some(config))
    }

    fn validate(&self) -> AppResult<()> {
        if self.api_key.trim().is_empty()
            || self.api_key.chars().any(char::is_control)
            || self.from.chars().any(char::is_control)
            || self.from.parse::<lettre::message::Mailbox>().is_err()
            || validation::email(&self.to).is_err()
        {
            return Err(AppError::ContactUnavailable);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactInput {
    pub name: String,
    pub email: String,
    pub subject: String,
    pub message: String,
    pub request_id: Uuid,
    #[serde(default)]
    pub website: String,
}

impl ContactInput {
    fn validate(&mut self) -> AppResult<()> {
        validation::text(&self.name, "Name", 2, 100)?;
        validation::text(&self.subject, "Subject", 2, 160)?;
        validation::text(&self.message, "Message", 10, 4000)?;
        if !self.website.is_empty()
            || self.name.chars().any(char::is_control)
            || self.subject.chars().any(char::is_control)
            || self
                .message
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || self.request_id.is_nil()
        {
            return Err(AppError::Invalid(
                "Please check your enquiry and try again".into(),
            ));
        }
        self.name = self.name.trim().into();
        self.subject = self.subject.trim().into();
        self.message = self.message.trim().into();
        self.email = validation::email(&self.email)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct ContactReceipt {
    pub id: Uuid,
}

#[derive(Default)]
struct Limits {
    global: Option<(Instant, u32)>,
    senders: HashMap<String, (Instant, u32)>,
}

impl Limits {
    fn check(&mut self, email: &str, now: Instant) -> AppResult<()> {
        let period = Duration::from_secs(60);
        self.senders
            .retain(|_, (start, _)| now.duration_since(*start) < period);
        let global = self.global.get_or_insert((now, 0));
        if now.duration_since(global.0) >= period {
            *global = (now, 0);
        }
        if global.1 >= 20 {
            return Err(AppError::ContactRateLimited);
        }
        let sender = self.senders.entry(email.into()).or_insert((now, 0));
        if sender.1 >= 3 {
            return Err(AppError::ContactRateLimited);
        }
        global.1 += 1;
        sender.1 += 1;
        Ok(())
    }
}

pub struct ContactService {
    config: Option<ContactConfig>,
    client: reqwest::Client,
    endpoint: String,
    limits: Mutex<Limits>,
    slots: Semaphore,
}

impl ContactService {
    pub fn new(config: Option<ContactConfig>) -> AppResult<Self> {
        if let Some(config) = &config {
            config.validate()?;
        }
        Ok(Self {
            config,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| AppError::Internal)?,
            endpoint: "https://api.resend.com/emails".into(),
            limits: Mutex::default(),
            slots: Semaphore::new(2),
        })
    }

    pub async fn send(&self, mut input: ContactInput) -> AppResult<ContactReceipt> {
        input.validate()?;
        let config = self.config.as_ref().ok_or(AppError::ContactUnavailable)?;
        self.limits
            .lock()
            .map_err(|_| AppError::Internal)?
            .check(&input.email, Instant::now())?;
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| AppError::ContactRateLimited)?;
        // The recipient and sender are server owned. Visitors only control Reply-To and plain text.
        let response = self.client.post(&self.endpoint)
            .bearer_auth(&config.api_key)
            .header("Idempotency-Key", format!("contact/{}", input.request_id))
            .json(&json!({
                "from": config.from,
                "to": [config.to],
                "reply_to": input.email,
                "subject": format!("Portfolio enquiry: {}", input.subject),
                "text": format!("Name: {}\nEmail: {}\n\n{}", input.name, input.email, input.message),
            }))
            .send().await.map_err(|_| AppError::ContactUnavailable)?;
        if !response.status().is_success() {
            // Log status only: provider responses can contain submitted data or configuration.
            tracing::warn!(
                status = response.status().as_u16(),
                "Resend rejected contact email"
            );
            return Err(AppError::ContactUnavailable);
        }
        let receipt = response
            .json::<ContactReceipt>()
            .await
            .map_err(|_| AppError::ContactUnavailable)?;
        if receipt.id.is_nil() {
            return Err(AppError::ContactUnavailable);
        }
        Ok(receipt)
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/contact", post(submit))
        .layer(DefaultBodyLimit::max(24 * 1024))
}

async fn submit(
    State(state): State<AppState>,
    ApiJson(input): ApiJson<ContactInput>,
) -> AppResult<Json<ContactReceipt>> {
    Ok(Json(state.contact.send(input).await?))
}

#[cfg(test)]
#[path = "contact_tests.rs"]
mod tests;
