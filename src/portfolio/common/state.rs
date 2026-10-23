use super::{
    config::Config,
    error::{AppError, AppResult},
    security::hash_password,
};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub password_jobs: Arc<Semaphore>,
    pub dummy_hash: Arc<String>,
    pub ai: Arc<crate::domains::ai::service::AiService>,
    attempts: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
}

impl AppState {
    pub async fn new(pool: PgPool, config: Config) -> AppResult<Self> {
        Ok(Self {
            ai: Arc::new(crate::domains::ai::service::AiService::new(
                config.ai.clone(),
            )?),
            pool,
            config,
            password_jobs: Arc::new(Semaphore::new(4)),
            dummy_hash: Arc::new(hash_password(super::security::token()).await?),
            attempts: Arc::default(),
        })
    }

    pub fn check_login_limit(&self, address: IpAddr) -> AppResult<()> {
        let now = Instant::now();
        let mut attempts = self.attempts.lock().map_err(|_| AppError::Internal)?;
        if attempts.len() >= 10_000 {
            attempts
                .retain(|_, (started, _)| now.duration_since(*started) < Duration::from_secs(60));
            if attempts.len() >= 10_000 && !attempts.contains_key(&address) {
                return Err(AppError::RateLimited);
            }
        }
        let window = attempts.entry(address).or_insert((now, 0));
        if now.duration_since(window.0) >= Duration::from_secs(60) {
            *window = (now, 0);
        }
        window.1 += 1;
        if window.1 > 10 {
            return Err(AppError::RateLimited);
        }
        Ok(())
    }
}
