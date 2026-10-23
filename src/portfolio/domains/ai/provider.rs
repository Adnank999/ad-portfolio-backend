use super::{
    config::AiConfig,
    dto::{GenerateRequest, GenerationUsage, ProviderDraft},
    prompt, schema, usage,
};
use crate::common::error::{AppError, AppResult};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

pub struct GeminiClient {
    client: Client,
    config: AiConfig,
    endpoint: String,
}

pub struct ProviderGeneration {
    pub draft: ProviderDraft,
    pub usage: Option<GenerationUsage>,
}
impl GeminiClient {
    pub fn new(config: AiConfig) -> AppResult<Self> {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            config.model
        );
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(config.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AppError::Internal)?;
        Ok(Self {
            client,
            config,
            endpoint,
        })
    }

    #[cfg(test)]
    pub fn mock(config: AiConfig, endpoint: String) -> AppResult<Self> {
        let mut client = Self::new(config)?;
        client.endpoint = endpoint;
        Ok(client)
    }

    pub async fn generate(&self, input: &GenerateRequest) -> AppResult<ProviderGeneration> {
        let body = json!({
            "systemInstruction":{"parts":[{"text":prompt::system(input.content_type)}]},
            "contents":[{"role":"user","parts":[{"text":prompt::user(input).map_err(|_| AppError::Internal)?}]}],
            "generationConfig":{"candidateCount":1,"maxOutputTokens":self.config.max_output_tokens,
                "responseFormat":{"text":{"mimeType":"APPLICATION_JSON","schema":schema::response(input.content_type,&input.target_fields)}}}
        });
        tokio::time::timeout(
            Duration::from_secs(self.config.timeout_secs),
            self.send(&body),
        )
        .await
        .map_err(|_| AppError::AiTimeout)?
    }

    async fn send(&self, body: &Value) -> AppResult<ProviderGeneration> {
        for attempt in 0..2 {
            let result = self
                .client
                .post(&self.endpoint)
                .header("x-goog-api-key", &self.config.api_key)
                .json(body)
                .send()
                .await;
            let mut response = match result {
                Ok(response) => response,
                Err(error) if error.is_connect() && attempt == 0 => {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    continue;
                }
                Err(error) if error.is_timeout() => return Err(AppError::AiTimeout),
                Err(_) => return Err(AppError::AiUnavailable),
            };
            let status = response.status();
            if !status.is_success() {
                tracing::warn!(
                    provider = "gemini",
                    model = %self.config.model,
                    status = status.as_u16(),
                    attempt = attempt + 1,
                    "AI provider rejected generation request"
                );
            }
            if status.as_u16() == 429 {
                return Err(AppError::AiRateLimited);
            }
            if matches!(status.as_u16(), 502..=504) && attempt == 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                continue;
            }
            if !status.is_success() {
                return Err(AppError::AiUnavailable);
            }
            if response
                .content_length()
                .is_some_and(|length| length > 512_000)
            {
                return Err(AppError::AiInvalidOutput);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| AppError::AiUnavailable)?
            {
                if bytes.len() + chunk.len() > 512_000 {
                    return Err(AppError::AiInvalidOutput);
                }
                bytes.extend_from_slice(&chunk);
            }
            let envelope: Value =
                serde_json::from_slice(&bytes).map_err(|_| AppError::AiInvalidOutput)?;
            if envelope
                .pointer("/candidates/0/finishReason")
                .and_then(Value::as_str)
                != Some("STOP")
            {
                return Err(AppError::AiInvalidOutput);
            }
            let parts = envelope
                .pointer("/candidates/0/content/parts")
                .and_then(Value::as_array)
                .ok_or(AppError::AiInvalidOutput)?;
            let text: String = parts
                .iter()
                .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            if text.is_empty() || text.len() > 256_000 {
                return Err(AppError::AiInvalidOutput);
            }
            let draft = serde_json::from_str(&text).map_err(|_| AppError::AiInvalidOutput)?;
            return Ok(ProviderGeneration {
                draft,
                usage: usage::from_envelope(
                    &envelope,
                    &self.config.model,
                    self.config.pricing.as_ref(),
                ),
            });
        }
        Err(AppError::AiUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, response::IntoResponse, routing::post, Router};

    fn input() -> GenerateRequest {
        serde_json::from_value(json!({"contentType":"blog","mode":"create","context":"Write about my Rust and PostgreSQL booking backend.","tone":"technical","audience":"software_developers","length":"medium","language":"English","targetFields":["title"],"contentContext":{"type":"blog"}})).unwrap()
    }
    fn config(timeout_secs: u64) -> AiConfig {
        AiConfig {
            api_key: "test-only-key".into(),
            model: "gemini-3.5-flash-lite".into(),
            timeout_secs,
            max_output_tokens: 1024,
            max_concurrent: 2,
            requests_per_hour: 20,
            pricing: super::super::usage::AiPricing::for_model("gemini-3.5-flash-lite"),
        }
    }
    async fn mock_server(
        status: StatusCode,
        body: String,
        delay: Duration,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/generate",post(move |axum::Json(payload): axum::Json<Value>| {
            let body = body.clone();
            async move {
                assert_eq!(payload.pointer("/generationConfig/responseFormat/text/mimeType").and_then(Value::as_str),Some("APPLICATION_JSON"));
                assert!(payload.pointer("/generationConfig/responseFormat/text/schema/properties/generatedFields/properties/title").is_some());
                tokio::time::sleep(delay).await;
                (status,body).into_response()
            }
        }));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}/generate"), task)
    }
    fn envelope(text: &str) -> String {
        json!({"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":text}]}}],"usageMetadata":{"promptTokenCount":1000,"candidatesTokenCount":400,"thoughtsTokenCount":100,"totalTokenCount":1500}})
            .to_string()
    }

    #[tokio::test]
    async fn valid_structured_response_and_safe_provider_errors() {
        for (status, body, expected) in [
            (
                StatusCode::OK,
                envelope(r#"{"generatedFields":{"title":"Rust booking backend"},"warnings":[]}"#),
                "ok",
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                "private provider error".into(),
                "rate",
            ),
            (
                StatusCode::UNAUTHORIZED,
                "private provider credentials".into(),
                "unavailable",
            ),
            (StatusCode::OK, envelope("not JSON"), "invalid"),
            (
                StatusCode::OK,
                envelope(r#"{"generatedFields":{"status":"published"},"warnings":[]}"#),
                "invalid",
            ),
        ] {
            let (endpoint, task) = mock_server(status, body, Duration::ZERO).await;
            let result = GeminiClient::mock(config(3), endpoint)
                .unwrap()
                .generate(&input())
                .await;
            task.abort();
            match expected {
                "ok" => {
                    let generated = result.unwrap();
                    assert_eq!(
                        generated.draft.generated_fields[&super::super::dto::Field::Title],
                        json!("Rust booking backend")
                    );
                    let usage = generated.usage.unwrap();
                    assert_eq!(usage.input_tokens, 1000);
                    assert_eq!(usage.output_tokens, 500);
                    assert_eq!(usage.thinking_tokens, 100);
                    assert!((usage.estimated_cost.unwrap().total_usd - 0.00155).abs() < 1e-12);
                }
                "rate" => assert!(matches!(result, Err(AppError::AiRateLimited))),
                "unavailable" => assert!(matches!(result, Err(AppError::AiUnavailable))),
                _ => assert!(matches!(result, Err(AppError::AiInvalidOutput))),
            }
        }
    }
    #[tokio::test]
    async fn provider_timeout_is_bounded() {
        let (endpoint, task) =
            mock_server(StatusCode::OK, String::new(), Duration::from_secs(3)).await;
        let result = GeminiClient::mock(config(1), endpoint)
            .unwrap()
            .generate(&input())
            .await;
        task.abort();
        assert!(matches!(result, Err(AppError::AiTimeout)));
    }

    #[tokio::test]
    async fn project_generation_and_one_transient_retry_use_the_same_safe_pipeline() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/generate", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/generate",
            post(move || {
                let attempts = observed.clone();
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return (StatusCode::SERVICE_UNAVAILABLE, String::new());
                    }
                    (
                        StatusCode::OK,
                        envelope(
                            r#"{"generatedFields":{"title":"Rust booking service"},"warnings":[]}"#,
                        ),
                    )
                }
            }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut input = input();
        input.content_type = super::super::dto::ContentType::Project;
        input.content_context = super::super::dto::ContentContext::Project(Default::default());
        let generated = GeminiClient::mock(config(4), endpoint)
            .unwrap()
            .generate(&input)
            .await
            .unwrap();
        let response =
            super::super::validation::output(&input, generated.draft, "https://portfolio.test")
                .unwrap();
        task.abort();
        assert_eq!(
            response.content_type,
            super::super::dto::ContentType::Project
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
