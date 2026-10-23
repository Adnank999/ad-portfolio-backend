use super::{
    error::{AppError, AppResult},
    state::AppState,
};
use argon2::{
    password_hash::{PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn token_hash(value: &str) -> AppResult<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::Unauthorized);
    }
    Ok(hex::encode(Sha256::digest(value.as_bytes())))
}

pub async fn hash_password(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || hash_sync(&password))
        .await
        .map_err(|_| AppError::Internal)?
}

fn hash_sync(password: &str) -> AppResult<String> {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let salt = SaltString::encode_b64(&bytes).map_err(|_| AppError::Internal)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::Internal)
}

pub async fn verify_password(
    state: &AppState,
    password: String,
    encoded: String,
) -> AppResult<bool> {
    let permit = state
        .password_jobs
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AppError::Internal)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = PasswordHash::new(&encoded).map_err(|_| AppError::Internal)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok())
    })
    .await
    .map_err(|_| AppError::Internal)?
}

pub async fn hash_password_limited(state: &AppState, password: String) -> AppResult<String> {
    let permit = state
        .password_jobs
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AppError::Internal)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_sync(&password)
    })
    .await
    .map_err(|_| AppError::Internal)?
}
