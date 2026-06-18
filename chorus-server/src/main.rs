use chorus_server::{app, metrics, state};

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chorus_core::Pipeline;
use chorus_core::backend::OpenAiBackend;
use chorus_core::config::Config;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use state::AppState;

fn load_config() -> anyhow::Result<Config> {
    let path = std::env::var("CHORUS_CONFIG").unwrap_or_else(|_| "config.toml".into());
    let config: Config = Figment::new()
        .merge(Toml::file(path))
        .merge(Env::prefixed("CHORUS_").split("__"))
        .extract()
        .context("load config")?;
    config.validate().context("validate config")?;
    Ok(config)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = load_config()?;
    let api_key = std::env::var(&config.backend.api_key_env)
        .with_context(|| format!("backend api key env {}", config.backend.api_key_env))?;

    let backend = Arc::new(OpenAiBackend::new(
        config.backend.base_url.clone(),
        api_key,
        Duration::from_millis(config.backend.timeout_ms),
    )?);

    let state = AppState {
        limiter: Arc::new(Semaphore::new(config.server.max_concurrent_requests)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: metrics::install(),
        config: Arc::new(config),
    };

    let bind = state.config.server.bind.clone();
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    tracing::info!(%bind, "chorus listening");

    axum::serve(listener, app::build(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
