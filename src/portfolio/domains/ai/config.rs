use super::usage::AiPricing;
use std::env;

#[derive(Clone)]
pub struct AiConfig {
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_output_tokens: u32,
    pub max_concurrent: usize,
    pub requests_per_hour: u32,
    pub pricing: Option<AiPricing>,
}
impl AiConfig {
    pub fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let key = env::var("AI_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            return Ok(None);
        }
        if env::var("AI_PROVIDER").unwrap_or_else(|_| "gemini".into()) != "gemini" {
            return Err("AI_PROVIDER must be gemini".into());
        }
        let model = env::var("AI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash-lite".into());
        let pricing = AiPricing::from_env(&model)?;
        let config = Self {
            api_key: key,
            model,
            pricing,
            timeout_secs: env::var("AI_REQUEST_TIMEOUT_SECS")
                .unwrap_or_else(|_| "90".into())
                .parse()?,
            max_output_tokens: env::var("AI_MAX_OUTPUT_TOKENS")
                .unwrap_or_else(|_| "8192".into())
                .parse()?,
            max_concurrent: env::var("AI_MAX_CONCURRENT_REQUESTS")
                .unwrap_or_else(|_| "2".into())
                .parse()?,
            requests_per_hour: env::var("AI_REQUESTS_PER_ADMIN_HOUR")
                .unwrap_or_else(|_| "20".into())
                .parse()?,
        };
        if !(10..=120).contains(&config.timeout_secs)
            || !(512..=16384).contains(&config.max_output_tokens)
            || !(1..=8).contains(&config.max_concurrent)
            || !(1..=100).contains(&config.requests_per_hour)
            || !config.model.starts_with("gemini-")
            || config.model.len() > 100
            || !config
                .model
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
        {
            return Err("AI configuration is outside permitted bounds".into());
        }
        Ok(Some(config))
    }
}
