//! Streaming: run the pipeline buffered, then stream the synthesized answer.
//!
//! The router, panel, and judge run buffered (they cannot stream). While they
//! run, the SSE stream emits keepalive comments so clients do not time out.
//! The final answer is emitted as `chat.completion.chunk` events followed by `[DONE]`.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chorus_core::config::Profile;
use chorus_core::schema::ChatCompletionRequest;
use futures::Stream;
use serde_json::json;
use tokio::time::interval;

use crate::state::AppState;

/// Stream the fused answer over SSE, emitting keepalive comments while the
/// buffered pipeline runs, then one `chat.completion.chunk` event and `[DONE]`.
#[allow(clippy::unused_async)]
pub async fn stream_fusion(
    state: AppState,
    profile: Profile,
    req: ChatCompletionRequest,
) -> Response {
    let model = format!("fusion/{}", profile.name);
    // Clone the Arc so we can move it into the stream without moving `state`.
    let pipeline = state.pipeline.clone();

    let stream = async_stream::stream! {
        let mut ticker = interval(Duration::from_secs(5));
        ticker.tick().await; // first tick fires immediately; skip it

        let fut = pipeline.run(&profile, &req);
        tokio::pin!(fut);

        let result = loop {
            tokio::select! {
                r = &mut fut => break r,
                _ = ticker.tick() => {
                    yield Ok::<Event, Infallible>(Event::default().comment("keepalive"));
                }
            }
        };

        match result {
            Ok(resp) => {
                let chunk = json!({
                    "id": resp.id,
                    "object": "chat.completion.chunk",
                    "created": resp.created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "content": resp.first_content() },
                        "finish_reason": "stop"
                    }]
                });
                yield Ok(Event::default().data(chunk.to_string()));
                yield Ok(Event::default().data("[DONE]"));
            }
            Err(e) => {
                let err = json!({
                    "error": { "message": e.to_string(), "type": "upstream_error" }
                });
                yield Ok(Event::default().data(err.to_string()));
                yield Ok(Event::default().data("[DONE]"));
            }
        }
    };

    sse_response(stream)
}

fn sse_response<S>(stream: S) -> Response
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
