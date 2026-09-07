mod api;
mod auth;
mod config;
mod db;
mod error;
mod indexer;
mod models;
mod state;

use axum::http::Method;
use state::AppState;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = config::Config::from_env();
    let pool = db::init_pool(&config.database_url).await?;

    let indexer_pool = pool.clone();
    let indexer_config = config.clone();
    tokio::spawn(async move {
        indexer::run(indexer_pool, indexer_config).await;
    });

    let state = AppState {
        pool,
        jwt_secret: config.jwt_secret.clone(),
        challenges: Arc::new(auth::ChallengeStore::new()),
    };

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = api::router()
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;
    tracing::info!(port = config.port, "concord-backend listening");
    axum::serve(listener, app).await?;

    Ok(())
}
