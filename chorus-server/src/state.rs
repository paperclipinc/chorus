//! Shared application state.

use std::sync::Arc;

use chorus_core::Pipeline;
use chorus_core::config::Config;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pipeline: Arc<Pipeline>,
    pub limiter: Arc<Semaphore>,
    pub metrics: PrometheusHandle,
}
