//! The router gate: decide whether to fuse or forward to a single model.

use async_trait::async_trait;

use crate::schema::ChatCompletionRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Fuse,
    Single,
}

#[async_trait]
pub trait Router: Send + Sync {
    async fn decide(&self, req: &ChatCompletionRequest) -> RouteDecision;
}

/// The M1 policy: always fuse. Removes the routing confound from the quality benchmark.
pub struct AlwaysFuse;

#[async_trait]
impl Router for AlwaysFuse {
    async fn decide(&self, _req: &ChatCompletionRequest) -> RouteDecision {
        RouteDecision::Fuse
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ChatMessage;

    #[tokio::test]
    async fn always_fuse_fuses() {
        let r = AlwaysFuse;
        let req = ChatCompletionRequest {
            model: "fusion/research".into(),
            messages: vec![ChatMessage::user("hi")],
            stream: false,
            temperature: None,
        };
        assert_eq!(r.decide(&req).await, RouteDecision::Fuse);
    }
}
