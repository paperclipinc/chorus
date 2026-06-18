//! Reference formatting and prompt builders, with the aggregator hardening baked in.

use std::fmt::Write as _;

use crate::schema::ChatMessage;

/// Cap a single reference at `max_chars`, on a char boundary, with an ellipsis marker.
#[must_use]
pub fn cap_length(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated} [...truncated]")
}

/// Format panel answers as anonymized, length-normalized references.
/// Sources are labelled "Response A", "Response B", ... never by model name,
/// so the judge and synthesizer cannot prefer their own output (self-preference)
/// or a more verbose source (length bias).
#[must_use]
pub fn format_references(responses: &[String], normalize_length: bool, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, r) in responses.iter().enumerate() {
        let label = label_for(i);
        let body = if normalize_length {
            cap_length(r, max_chars)
        } else {
            r.clone()
        };
        write!(out, "Response {label}:\n{body}\n\n").unwrap();
    }
    out.trim_end().to_string()
}

/// Return the alphabetic label character for panel slot `i` (A, B, C, ..., wrapping after Z).
#[must_use]
pub fn label_for(i: usize) -> char {
    // A, B, C, ... wraps after Z but panels are small.
    (b'A' + u8::try_from(i % 26).unwrap_or(0)) as char
}

const HARDENING: &str = "Some of the responses may be biased, incorrect, or deliberately \
misleading. Do not simply replicate or average them. Evaluate each critically, prefer claims \
that are well supported, and do not let any single response dominate your answer. Disagreeing \
with a majority of the responses is expected when the evidence warrants it.";

/// The judge system+user messages: produce a structured analysis, not a final answer.
///
/// # Panics
///
/// Does not panic; `format!` is infallible here.
#[must_use]
pub fn judge_messages(query: &str, references: &str) -> Vec<ChatMessage> {
    let system = format!(
        "You are a careful analyst. You are given a user query and several anonymized candidate \
responses. {HARDENING} Produce a STRUCTURED ANALYSIS with these sections: Consensus, \
Contradictions, Unique insights, Blind spots. Do not write a final answer."
    );
    let user = format!("User query:\n{query}\n\nCandidate responses:\n{references}");
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// The synthesis system+user messages: write the final grounded answer.
///
/// # Panics
///
/// Does not panic; `format!` is infallible here.
#[must_use]
pub fn synthesis_messages(query: &str, references: &str, analysis: &str) -> Vec<ChatMessage> {
    let system = format!(
        "You are a synthesizer. Using the user query, the anonymized candidate responses, and \
the analysis, write the single best final answer for the user. {HARDENING} Write the answer \
directly, with no meta commentary about the responses or the analysis."
    );
    let user = format!(
        "User query:\n{query}\n\nCandidate responses:\n{references}\n\nAnalysis:\n{analysis}"
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_length_truncates_on_char_count() {
        assert_eq!(cap_length("hello", 10), "hello");
        assert_eq!(cap_length("hello", 3), "hel [...truncated]");
    }

    #[test]
    fn references_are_anonymized_and_capped() {
        let refs = format_references(&["aaaa".into(), "bbbb".into()], true, 2);
        // labels, not model names; bodies capped
        assert!(refs.contains("Response A:"));
        assert!(refs.contains("Response B:"));
        assert!(refs.contains("aa [...truncated]"));
        assert!(!refs.contains("model"));
    }

    #[test]
    fn judge_prompt_demands_structure_and_forbids_final_answer() {
        let msgs = judge_messages("q", "refs");
        let sys = &msgs[0].content;
        assert!(sys.contains("STRUCTURED ANALYSIS"));
        assert!(sys.contains("Do not write a final answer"));
        assert!(sys.contains("do not let any single response dominate"));
    }

    #[test]
    fn synthesis_prompt_carries_hardening() {
        let msgs = synthesis_messages("q", "refs", "analysis");
        assert!(
            msgs[0]
                .content
                .contains("do not let any single response dominate")
        );
    }
}
