//! Aggregate token usage across pipeline stages.

use crate::schema::Usage;

/// Accumulates [`Usage`] across multiple pipeline stages, skipping absent values.
#[derive(Debug, Default, Clone)]
pub struct UsageAccumulator {
    total: Usage,
}

impl UsageAccumulator {
    /// Add a usage value to the accumulator. `None` is silently skipped.
    pub fn add(&mut self, u: Option<&Usage>) {
        if let Some(u) = u {
            self.total.prompt_tokens += u.prompt_tokens;
            self.total.completion_tokens += u.completion_tokens;
            self.total.total_tokens += u.total_tokens;
        }
    }

    /// Consume the accumulator and return the aggregated [`Usage`].
    #[must_use]
    pub fn into_usage(self) -> Usage {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_present_skips_absent() {
        let mut acc = UsageAccumulator::default();
        acc.add(Some(&Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
        }));
        acc.add(None);
        acc.add(Some(&Usage {
            prompt_tokens: 4,
            completion_tokens: 5,
            total_tokens: 9,
        }));
        let u = acc.into_usage();
        assert_eq!(
            u,
            Usage {
                prompt_tokens: 5,
                completion_tokens: 7,
                total_tokens: 12
            }
        );
    }
}
