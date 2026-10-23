use portfolio_api::{
    app::create_router,
    common::{config::Config, state::AppState},
    domains::auth::service::bootstrap_owner,
    MIGRATOR,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    if matches!(command.as_str(), "--help" | "-h") {
        println!(
            "portfolio-api [serve|migrate|bootstrap-owner|prune-sessions|import-case-studies|import-services]"
        );
        return Ok(());
    }
    if !matches!(
        command.as_str(),
        "serve"
            | "migrate"
            | "bootstrap-owner"
            | "prune-sessions"
            | "import-case-studies"
            | "import-services"
    ) {
        return Err("Unknown command; use --help".into());
    }
    let config = Config::from_env()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();
    let pool = config.connect().await?;
    if command == "migrate" {
        MIGRATOR.run(&pool).await?;
        tracing::info!("Portfolio migrations applied");
        return Ok(());
    }
    if command == "prune-sessions" {
        let result=sqlx::query("DELETE FROM auth_sessions WHERE refresh_expires_at<now() OR revoked_at<now()-interval '7 days'").execute(&pool).await?;
        tracing::info!(removed = result.rows_affected(), "Expired sessions pruned");
        return Ok(());
    }
    if command == "import-case-studies" {
        let imported = portfolio_api::domains::content::import::import_case_studies(&pool).await?;
        tracing::info!(
            imported,
            "Case studies imported; existing records left unchanged"
        );
        return Ok(());
    }
    if command == "import-services" {
        let imported = portfolio_api::domains::content::import::import_services(&pool).await?;
        tracing::info!(
            imported,
            "Services imported; existing records left unchanged"
        );
        return Ok(());
    }
    let state = AppState::new(pool, config.clone()).await?;
    if command == "bootstrap-owner" {
        bootstrap_owner(
            &state,
            std::env::var("BOOTSTRAP_OWNER_EMAIL")?,
            std::env::var("BOOTSTRAP_OWNER_NAME")?,
            std::env::var("BOOTSTRAP_OWNER_PASSWORD")?,
        )
        .await?;
        tracing::info!("Owner created; credentials were not logged");
        return Ok(());
    }
    let address = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address,"Portfolio API listening");
    axum::serve(
        listener,
        create_router(state)?.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
