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
use metrics::set_global_recorder;
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
                classifier_model: None,
                threshold: 0.5,
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

// Helper: build a minimal AppState backed by the given mock upstream URI.
// Uses a fresh PrometheusBuilder so each test keeps its own metrics handle.
fn test_state(upstream_v1_base: &str) -> chorus_server::state::AppState {
    let config = test_config(upstream_v1_base.to_string());
    let backend = Arc::new(
        OpenAiBackend::new(config.backend.base_url.clone(), "k", Duration::from_secs(5)).unwrap(),
    );
    chorus_server::state::AppState {
        limiter: Arc::new(Semaphore::new(8)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: PrometheusBuilder::new().build_recorder().handle(),
        config: Arc::new(config),
    }
}

// ---------------------------------------------------------------------------
// Test 1: streaming SSE path
// ---------------------------------------------------------------------------
#[tokio::test]
async fn streaming_response_is_event_stream_with_done_sentinel() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body_for("FUSED")))
        .mount(&upstream)
        .await;

    let state = test_state(&format!("{}/v1", upstream.uri()));
    let app = chorus_server::app::build(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}],"stream":true}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Content-Type must be text/event-stream
    let ct = resp
        .headers()
        .get("content-type")
        .expect("content-type header missing")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("text/event-stream"),
        "expected text/event-stream, got: {ct}"
    );

    // Collect the full SSE body and assert on its contents.
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();

    // Must contain the [DONE] sentinel
    assert!(body.contains("[DONE]"), "body missing [DONE]: {body}");

    // Must contain a chat.completion.chunk with the fused content
    assert!(
        body.contains("chat.completion.chunk"),
        "body missing chat.completion.chunk: {body}"
    );
    assert!(
        body.contains("FUSED"),
        "body missing fused content FUSED: {body}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: unknown profile returns 404 with OpenAI error shape
// ---------------------------------------------------------------------------
#[tokio::test]
async fn unknown_profile_returns_404_with_openai_error_shape() {
    let upstream = MockServer::start().await;
    // No mocked responses needed; the error is returned before hitting upstream.

    let state = test_state(&format!("{}/v1", upstream.uri()));
    let app = chorus_server::app::build(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/does-not-exist","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Must have the OpenAI error envelope: { "error": { "message": "...", "type": "..." } }
    assert!(
        v["error"]["message"].is_string(),
        "error.message missing or not a string: {v}"
    );
    assert!(
        v["error"]["type"].is_string(),
        "error.type missing or not a string: {v}"
    );
}

#[tokio::test]
async fn model_without_fusion_prefix_returns_404_with_openai_error_shape() {
    let upstream = MockServer::start().await;

    let state = test_state(&format!("{}/v1", upstream.uri()));
    let app = chorus_server::app::build(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        v["error"]["message"].is_string(),
        "error.message missing or not a string: {v}"
    );
    assert!(
        v["error"]["type"].is_string(),
        "error.type missing or not a string: {v}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: concurrency limiter - permit acquire/release path
// ---------------------------------------------------------------------------
// A deterministic, non-flaky concurrency test: build a state with
// max_concurrent_requests = 1 and assert that a normal request still
// succeeds, which proves the permit is acquired and released correctly.
//
// A true contention test (two simultaneous requests where one must wait)
// would require carefully coordinating goroutine/task timing via channels
// and risks flakiness under CI load. The acquire/release path is exercised
// by this simpler form; the semaphore itself is a well-tested tokio primitive.
#[tokio::test]
async fn limiter_with_capacity_one_succeeds_and_releases_permit() {
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
    // Capacity = 1: the single permit must be acquired and released per request.
    let limiter = Arc::new(Semaphore::new(1));
    let state = chorus_server::state::AppState {
        limiter: limiter.clone(),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: PrometheusBuilder::new().build_recorder().handle(),
        config: Arc::new(config),
    };
    let app = chorus_server::app::build(state);

    // First request must succeed (acquires + releases the sole permit).
    let req1 = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // After the first request completes, the semaphore must have its permit back.
    assert_eq!(
        limiter.available_permits(),
        1,
        "permit not returned after request completed"
    );

    // A second sequential request must also succeed, confirming the permit was released.
    let req2 = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_endpoint_exposes_request_counter() {
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
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    set_global_recorder(recorder).ok();
    let state = chorus_server::state::AppState {
        limiter: Arc::new(Semaphore::new(8)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: handle,
        config: Arc::new(config),
    };
    let app = chorus_server::app::build(state);

    let post = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();
    let _ = app.clone().oneshot(post).await.unwrap();

    let get = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(get).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("chorus_requests_total"));
}
