use super::{
    dto::{ChangePassword, ForgotPassword, Login, Refresh, Register, ResetPassword, Tokens},
    repository, Principal,
};
use crate::common::{
    error::{AppError, AppResult},
    security,
    state::AppState,
    validation,
};
use chrono::{Duration, Utc};
use lettre::{
    transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport, Message,
    Tokio1Executor,
};
use sqlx::Row;
use uuid::Uuid;

pub async fn register(state: &AppState, input: Register) -> AppResult<()> {
    let email = validation::email(&input.email)?;
    validation::text(&input.name, "name", 1, 120)?;
    validation::password(&input.password)?;
    let hash = security::hash_password_limited(state, input.password).await?;
    sqlx::query("INSERT INTO users(id,email,name,password_hash) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(email)
        .bind(input.name.trim())
        .bind(hash)
        .execute(&state.pool)
        .await?;
    Ok(())
}

pub async fn forgot_password(state: &AppState, input: ForgotPassword) -> AppResult<()> {
    let email_config = state
        .config
        .reset_email
        .as_ref()
        .ok_or(AppError::Unavailable)?;
    let email = validation::email(&input.email)?;
    let user_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE email=$1 AND active")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;
    let Some(user_id) = user_id else {
        return Ok(());
    };
    let token = security::token();
    let token_hash = security::token_hash(&token)?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query("DELETE FROM password_reset_tokens WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO password_reset_tokens(token_hash,user_id,expires_at) VALUES($1,$2,$3)",
    )
    .bind(&token_hash)
    .bind(user_id)
    .bind(Utc::now() + Duration::minutes(15))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    let url = format!("{}/reset-password?token={token}", state.config.frontend_url);
    let message = Message::builder()
        .from(email_config.from.parse().map_err(|_| AppError::Internal)?)
        .to(email.parse().map_err(|_| AppError::Internal)?)
        .subject("Reset your portfolio password")
        .body(format!("Use this link to reset your password. It expires in 15 minutes:\n\n{url}\n\nIf you did not request this, ignore this email."))
        .map_err(|_| AppError::Internal)?;
    let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&email_config.host)
        .map_err(|_| AppError::Internal)?
        .port(email_config.port)
        .credentials(Credentials::new(
            email_config.username.clone(),
            email_config.password.clone(),
        ))
        .timeout(Some(std::time::Duration::from_secs(10)))
        .build();
    if let Err(error) = transport.send(message).await {
        tracing::error!(%error, "Password reset email delivery failed");
        sqlx::query("DELETE FROM password_reset_tokens WHERE token_hash=$1")
            .bind(token_hash)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

pub async fn reset_password(state: &AppState, input: ResetPassword) -> AppResult<()> {
    validation::password(&input.password)?;
    let token_hash = security::token_hash(&input.token)
        .map_err(|_| AppError::Invalid("Invalid or expired reset link".into()))?;
    let password_hash = security::hash_password_limited(state, input.password).await?;
    let mut transaction = state.pool.begin().await?;
    let user_id: Option<Uuid> = sqlx::query_scalar("SELECT p.user_id FROM password_reset_tokens p JOIN users u ON u.id=p.user_id WHERE p.token_hash=$1 AND p.used_at IS NULL AND p.expires_at>now() AND u.active FOR UPDATE OF p")
        .bind(&token_hash)
        .fetch_optional(&mut *transaction)
        .await?;
    let user_id =
        user_id.ok_or_else(|| AppError::Invalid("Invalid or expired reset link".into()))?;
    sqlx::query("UPDATE users SET password_hash=$1,updated_at=now() WHERE id=$2")
        .bind(password_hash)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE auth_sessions SET revoked_at=now() WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE password_reset_tokens SET used_at=now() WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn login(state: &AppState, input: Login) -> AppResult<Tokens> {
    let email = validation::email(&input.email).map_err(|_| AppError::Unauthorized)?;
    if input.password.len() > 512 {
        return Err(AppError::Unauthorized);
    }
    let user =
        sqlx::query("SELECT id,email,name,headline,password_hash,active FROM users WHERE email=$1")
            .bind(email)
            .fetch_optional(&state.pool)
            .await?;
    let hash = match &user {
        Some(row) => row.try_get::<String, _>("password_hash")?,
        None => state.dummy_hash.as_ref().clone(),
    };
    let valid = security::verify_password(state, input.password, hash.clone()).await?;
    let row = user.ok_or(AppError::Unauthorized)?;
    if !valid || !row.try_get::<bool, _>("active")? {
        return Err(AppError::Unauthorized);
    }
    let id: Uuid = row.try_get("id")?;
    let session_id = Uuid::new_v4();
    let access_token = security::token();
    let refresh_token = security::token();
    let mut transaction = state.pool.begin().await?;
    let created = sqlx::query("INSERT INTO auth_sessions(id,user_id,token_hash,refresh_hash,access_expires_at,refresh_expires_at) SELECT $1,id,$2,$3,$4,$5 FROM users WHERE id=$6 AND active AND password_hash=$7 FOR UPDATE")
        .bind(session_id).bind(security::token_hash(&access_token)?).bind(security::token_hash(&refresh_token)?).bind(Utc::now()+Duration::minutes(state.config.access_minutes)).bind(Utc::now()+Duration::days(state.config.refresh_days)).bind(id).bind(hash).execute(&mut *transaction).await?;
    if created.rows_affected() != 1 {
        return Err(AppError::Unauthorized);
    }
    let user = repository::principal(
        &mut transaction,
        id,
        session_id,
        row.try_get("email")?,
        row.try_get("name")?,
        row.try_get("headline")?,
    )
    .await?;
    transaction.commit().await?;
    Ok(Tokens {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: state.config.access_minutes * 60,
        user,
    })
}

pub async fn refresh(state: &AppState, input: Refresh) -> AppResult<Tokens> {
    let hash = security::token_hash(&input.refresh_token)?;
    let mut transaction = state.pool.begin().await?;
    let row = sqlx::query("SELECT s.id AS session_id,u.id,u.email,u.name,u.headline,u.active,s.refresh_expires_at,s.revoked_at FROM auth_sessions s JOIN users u ON u.id=s.user_id WHERE s.refresh_hash=$1 FOR UPDATE OF s")
        .bind(&hash).fetch_optional(&mut *transaction).await?;
    let Some(row) = row else {
        sqlx::query("UPDATE auth_sessions SET revoked_at=now() WHERE id IN (SELECT session_id FROM used_refresh_tokens WHERE token_hash=$1)").bind(hash).execute(&mut *transaction).await?;
        transaction.commit().await?;
        return Err(AppError::Unauthorized);
    };
    if !row.try_get::<bool, _>("active")?
        || row
            .try_get::<Option<chrono::DateTime<Utc>>, _>("revoked_at")?
            .is_some()
        || row.try_get::<chrono::DateTime<Utc>, _>("refresh_expires_at")? <= Utc::now()
    {
        return Err(AppError::Unauthorized);
    }
    let session_id: Uuid = row.try_get("session_id")?;
    let now = Utc::now();
    let access_deadline = (now + Duration::minutes(state.config.access_minutes))
        .min(row.try_get("refresh_expires_at")?);
    let access_token = security::token();
    let refresh_token = security::token();
    sqlx::query("INSERT INTO used_refresh_tokens(token_hash,session_id) VALUES ($1,$2)")
        .bind(hash)
        .bind(session_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE auth_sessions SET token_hash=$1,refresh_hash=$2,access_expires_at=$3 WHERE id=$4",
    )
    .bind(security::token_hash(&access_token)?)
    .bind(security::token_hash(&refresh_token)?)
    .bind(access_deadline)
    .bind(session_id)
    .execute(&mut *transaction)
    .await?;
    let user = repository::principal(
        &mut transaction,
        row.try_get("id")?,
        session_id,
        row.try_get("email")?,
        row.try_get("name")?,
        row.try_get("headline")?,
    )
    .await?;
    transaction.commit().await?;
    Ok(Tokens {
        access_token,
        refresh_token,
        token_type: "Bearer",
        expires_in: (access_deadline - now).num_seconds(),
        user,
    })
}

pub async fn change_password(
    state: &AppState,
    user: &Principal,
    input: ChangePassword,
) -> AppResult<()> {
    validation::password(&input.new_password)?;
    if input.current_password.len() > 512 {
        return Err(AppError::Unauthorized);
    }
    let current: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 AND active")
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::Unauthorized)?;
    if !security::verify_password(state, input.current_password, current.clone()).await? {
        return Err(AppError::Unauthorized);
    }
    let updated = security::hash_password_limited(state, input.new_password).await?;
    let mut transaction = state.pool.begin().await?;
    let result = sqlx::query("UPDATE users SET password_hash=$1,updated_at=now() WHERE id=$2 AND password_hash=$3 AND active").bind(updated).bind(user.id).bind(current).execute(&mut *transaction).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Conflict("Account changed; sign in again".into()));
    }
    sqlx::query("UPDATE auth_sessions SET revoked_at=now() WHERE user_id=$1")
        .bind(user.id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn bootstrap_owner(
    state: &AppState,
    email: String,
    name: String,
    password: String,
) -> AppResult<()> {
    let email = validation::email(&email)?;
    validation::text(&name, "name", 1, 120)?;
    validation::password(&password)?;
    let hash = security::hash_password_limited(state, password).await?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(723422105)")
        .execute(&mut *transaction)
        .await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN roles r ON r.id=ur.role_id WHERE r.name='owner')").fetch_one(&mut *transaction).await?;
    if exists {
        return Err(AppError::Conflict(
            "An owner already exists; bootstrap cannot reset or replace it".into(),
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,name,headline,password_hash) VALUES($1,$2,$3,'Full Stack Developer',$4)").bind(id).bind(email).bind(name.trim()).bind(hash).execute(&mut *transaction).await?;
    sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='owner'")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
