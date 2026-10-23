use super::error::{AppError, AppResult};

pub fn text(value: &str, field: &str, minimum: usize, maximum: usize) -> AppResult<()> {
    let length = value.trim().chars().count();
    if length < minimum || length > maximum || value.contains('\0') {
        return Err(AppError::Invalid(format!(
            "{field} must contain {minimum} to {maximum} characters"
        )));
    }
    Ok(())
}

pub fn slug(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 120
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AppError::Invalid("Slug must contain lowercase words or digits separated by single hyphens (maximum 120 characters)".into()));
    }
    Ok(())
}

pub fn email(value: &str) -> AppResult<String> {
    let value = value.trim().to_lowercase();
    text(&value, "email", 3, 254)?;
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    let local_valid = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&byte));
    let domain_valid = domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    if !local_valid || !domain_valid || parts.next().is_some() {
        return Err(AppError::Invalid(
            "A valid email address is required".into(),
        ));
    }
    Ok(value)
}

pub fn password(value: &str) -> AppResult<()> {
    text(value, "password", 12, 128)
}

pub fn url(value: &str, field: &str, relative: bool, optional: bool) -> AppResult<()> {
    if optional && value.is_empty() {
        return Ok(());
    }
    text(value, field, 1, 2048)?;
    if value.chars().any(char::is_control) || value.contains('\\') {
        return Err(AppError::Invalid(format!(
            "{field} contains an invalid URL"
        )));
    }
    if relative && value.starts_with('/') && !value.starts_with("//") {
        return Ok(());
    }
    let parsed = url::Url::parse(value).map_err(|_| {
        AppError::Invalid(format!(
            "{field} must be an HTTP(S) URL{}",
            if relative {
                " or root-relative path"
            } else {
                ""
            }
        ))
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::Invalid(format!(
            "{field} must be an HTTP(S) URL without credentials"
        )));
    }
    Ok(())
}

pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
