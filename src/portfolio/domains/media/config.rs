use std::env;

#[derive(Clone)]
pub struct R2Config {
    pub account_id: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub staging_bucket: String,
    pub public_bucket: String,
    pub public_base_url: String,
    pub ttl_seconds: u64,
}

impl R2Config {
    pub fn from_env() -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        if env::var("R2_DIRECT_UPLOAD_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        let config = Self {
            account_id: env::var("R2_ACCOUNT_ID")?,
            access_key_id: env::var("R2_ACCESS_KEY_ID")?,
            secret_access_key: env::var("R2_SECRET_ACCESS_KEY")?,
            staging_bucket: env::var("R2_STAGING_BUCKET")?,
            public_bucket: env::var("R2_PUBLIC_BUCKET")?,
            public_base_url: env::var("R2_PUBLIC_BASE_URL")?,
            ttl_seconds: env::var("R2_PRESIGN_TTL_SECONDS")
                .unwrap_or_else(|_| "300".into())
                .parse()?,
        };
        config.validate()?;
        Ok(Some(config))
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let public_url = url::Url::parse(&self.public_base_url)?;
        if self.account_id.len() != 32
            || !self
                .account_id
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
            || self.access_key_id.is_empty()
            || self.secret_access_key.is_empty()
            || self.staging_bucket == self.public_bucket
            || !(30..=300).contains(&self.ttl_seconds)
            || public_url.scheme() != "https"
            || public_url.host_str().is_none()
            || !public_url.username().is_empty()
            || public_url.password().is_some()
            || public_url.path() != "/"
            || public_url.query().is_some()
            || public_url.fragment().is_some()
        {
            return Err("Invalid private/public R2 upload configuration".into());
        }
        for bucket in [&self.staging_bucket, &self.public_bucket] {
            if !(3..=63).contains(&bucket.len())
                || !bucket.bytes().all(|value| {
                    value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-'
                })
            {
                return Err("Invalid R2 bucket name".into());
            }
        }
        Ok(())
    }
}
