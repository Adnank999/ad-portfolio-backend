use super::{config::AiConfig, dto::*, provider::GeminiClient, validation};
use crate::{
    common::error::{AppError, AppResult},
    domains::auth::Principal,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;

struct Window {
    started: Instant,
    count: u32,
    active: bool,
}
pub struct AiService {
    provider: Option<GeminiClient>,
    jobs: Arc<Semaphore>,
    windows: Arc<Mutex<HashMap<Uuid, Window>>>,
    requests_per_hour: u32,
}

struct GenerationPermit {
    _slot: OwnedSemaphorePermit,
    windows: Arc<Mutex<HashMap<Uuid, Window>>>,
    user: Uuid,
}
impl Drop for GenerationPermit {
    fn drop(&mut self) {
        if let Ok(mut windows) = self.windows.lock() {
            if let Some(window) = windows.get_mut(&self.user) {
                window.active = false;
            }
        }
    }
}
pub fn authorize(user: &Principal, kind: ContentType) -> AppResult<()> {
    if !user
        .roles
        .iter()
        .any(|role| matches!(role.as_str(), "owner" | "admin"))
    {
        return Err(AppError::Forbidden);
    }
    user.require(match kind {
        ContentType::Blog => "blogs:write",
        ContentType::Project => "projects:write",
        ContentType::CaseStudy => "case-studies:write",
        ContentType::Service => "services:write",
    })
}
impl AiService {
    pub fn new(config: Option<AiConfig>) -> AppResult<Self> {
        let jobs = config
            .as_ref()
            .map(|config| config.max_concurrent)
            .unwrap_or(1);
        let requests_per_hour = config
            .as_ref()
            .map(|config| config.requests_per_hour)
            .unwrap_or(20);
        Ok(Self {
            provider: config.map(GeminiClient::new).transpose()?,
            jobs: Arc::new(Semaphore::new(jobs)),
            windows: Arc::default(),
            requests_per_hour,
        })
    }
    fn begin(&self, user: Uuid) -> AppResult<GenerationPermit> {
        let slot = self
            .jobs
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::AiBusy)?;
        let now = Instant::now();
        let mut windows = self.windows.lock().map_err(|_| AppError::Internal)?;
        windows.retain(|_, window| {
            window.active || now.duration_since(window.started) < Duration::from_secs(3600)
        });
        if windows.len() >= 10_000 && !windows.contains_key(&user) {
            return Err(AppError::AiBusy);
        }
        let window = windows.entry(user).or_insert(Window {
            started: now,
            count: 0,
            active: false,
        });
        if window.active {
            return Err(AppError::AiBusy);
        }
        if window.count >= self.requests_per_hour {
            return Err(AppError::AiRateLimited);
        }
        window.count += 1;
        window.active = true;
        Ok(GenerationPermit {
            _slot: slot,
            windows: self.windows.clone(),
            user,
        })
    }
    pub async fn generate(
        &self,
        user: &Principal,
        mut input: GenerateRequest,
        origin: &str,
    ) -> AppResult<GenerateResponse> {
        authorize(user, input.content_type)?;
        validation::request(&input)?;
        let original = input.clone();
        if input.mode == Mode::FillMissing {
            input
                .target_fields
                .retain(|field| validation::empty(input.existing_fields.get(field.key())));
        }
        if input.target_fields.iter().all(|field| field.derived()) {
            return validation::output(
                &original,
                ProviderDraft {
                    generated_fields: Default::default(),
                    warnings: Vec::new(),
                },
                origin,
            );
        }
        let provider = self.provider.as_ref().ok_or(AppError::AiUnconfigured)?;
        let _permit = self.begin(user.id)?;
        let started = Instant::now();
        let result = provider.generate(&input).await.and_then(|generated| {
            let mut response = validation::output(&original, generated.draft, origin)?;
            response.usage = generated.usage;
            Ok(response)
        });
        tracing::info!(user_id=%user.id,elapsed_ms=started.elapsed().as_millis(),success=result.is_ok(),error=result.as_ref().err().map(ToString::to_string),"AI content generation completed");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_admin_quota_concurrency_and_dropped_request_cleanup() {
        let mut service = AiService::new(None).unwrap();
        service.requests_per_hour = 2;
        let user = Uuid::new_v4();
        let first = service.begin(user).unwrap();
        assert!(matches!(service.begin(user), Err(AppError::AiBusy)));
        assert!(matches!(
            service.begin(Uuid::new_v4()),
            Err(AppError::AiBusy)
        ));
        drop(first);
        drop(service.begin(user).unwrap());
        assert!(matches!(service.begin(user), Err(AppError::AiRateLimited)));
        assert!(service.begin(Uuid::new_v4()).is_ok());
    }
}
