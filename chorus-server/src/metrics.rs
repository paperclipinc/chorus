//! Prometheus recorder install and handle for the /metrics endpoint.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Install a global Prometheus recorder and return a handle for scraping.
///
/// # Panics
///
/// Panics if the recorder cannot be installed (e.g. already installed).
#[must_use]
pub fn install() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("install prometheus recorder")
}
