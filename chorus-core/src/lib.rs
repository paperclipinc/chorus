//! chorus-core: the backend-agnostic Mixture-of-Agents engine.

pub mod error;
pub use error::Error;

pub mod schema;
pub use schema::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, Usage};
