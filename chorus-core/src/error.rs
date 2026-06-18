//! The single error type for the engine.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("backend request failed: {0}")]
    Backend(String),
    #[error("backend timed out")]
    Timeout,
    #[error("quorum not met: {got} of {needed} panel members returned")]
    Quorum { got: usize, needed: usize },
    #[error("synthesis failed: {0}")]
    Synthesis(String),
    #[error("unknown profile: {0}")]
    UnknownProfile(String),
    #[error("invalid model alias: {0}")]
    InvalidModel(String),
    #[error("config error: {0}")]
    Config(String),
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn quorum_error_renders_counts() {
        let e = Error::Quorum { got: 1, needed: 2 };
        assert_eq!(
            e.to_string(),
            "quorum not met: 1 of 2 panel members returned"
        );
    }
}
