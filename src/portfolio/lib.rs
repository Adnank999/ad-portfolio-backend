pub mod app;
pub mod common;
pub mod domains;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
