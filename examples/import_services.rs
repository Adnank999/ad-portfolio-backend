use portfolio_api::{common::config::Config, domains::content::import::import_services, MIGRATOR};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env()?;
    let pool = config.connect().await?;
    MIGRATOR.run(&pool).await?;
    let count = import_services(&pool).await?;
    println!("Imported {count} services; existing records left unchanged.");
    Ok(())
}
