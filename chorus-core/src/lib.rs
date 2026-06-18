//! chorus-core: the backend-agnostic Mixture-of-Agents engine.

pub mod error;
pub use error::Error;

pub mod schema;
pub use schema::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, Usage};

pub mod backend;
pub use backend::{ChatBackend, OpenAiBackend};

pub mod config;
pub use config::{Config, Profile};

pub mod router;
pub use router::{RouteDecision, Router};

pub mod prompts;

pub mod usage;
pub use usage::UsageAccumulator;

pub mod panel;
pub use panel::{PanelOutcome, run_panel};

pub mod judge;
pub use judge::{JudgeOutcome, run_judge};

pub mod synthesis;
pub use synthesis::{run_synthesis, synthesis_request};

pub mod pipeline;
pub use pipeline::Pipeline;
