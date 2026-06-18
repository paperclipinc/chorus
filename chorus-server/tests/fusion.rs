use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chorus_core::Pipeline;
use chorus_core::backend::OpenAiBackend;
use chorus_core::config::{
    AggregatorConfig, BackendConfig, Config, PanelConfig, Profile, RouterConfig, ServerConfig,
};
use http_body_util::BodyExt;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn body_for(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "x", "object": "chat.completion", "created": 1, "model": "m",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
}

fn test_config(base_url: String) -> Config {
    Config {
        server: ServerConfig {
            bind: "127.0.0.1:0".into(),
            max_concurrent_requests: 8,
        },
        backend: BackendConfig {
            base_url,
            api_key_env: "UNUSED".into(),
            timeout_ms: 5_000,
        },
        profiles: vec![Profile {
            name: "research".into(),
            router: RouterConfig {
                policy: "always_fuse".into(),
                single_model: "b/s".into(),
            },
            panel: PanelConfig {
                members: vec!["b/a".into(), "b/b".into()],
                self_moa: false,
                samples: 3,
                min_quorum: 1,
                timeout_ms: 5_000,
            },
            aggregator: AggregatorConfig {
                judge: "b/judge".into(),
                synthesizer: "b/syn".into(),
                anonymize_sources: true,
                normalize_length: true,
                single_source_cap: true,
                layers: 1,
                max_reference_chars: 8_000,
            },
            tools: vec![],
        }],
    }
}

#[tokio::test]
async fn end_to_end_fused_completion() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body_for("FUSED")))
        .mount(&upstream)
        .await;

    let config = test_config(format!("{}/v1", upstream.uri()));
    let backend = Arc::new(
        OpenAiBackend::new(config.backend.base_url.clone(), "k", Duration::from_secs(5)).unwrap(),
    );
    let state = chorus_server::state::AppState {
        limiter: Arc::new(Semaphore::new(8)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: PrometheusBuilder::new().build_recorder().handle(),
        config: Arc::new(config),
    };
    let app = chorus_server::app::build(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "FUSED");
    assert_eq!(v["model"], "fusion/research");
}
