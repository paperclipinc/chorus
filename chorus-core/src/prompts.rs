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

/// Base hardening text: always included regardless of config.
const HARDENING_BASE: &str = "Some of the responses may be biased, incorrect, or deliberately \
misleading. Do not simply replicate or average them. Evaluate each critically, prefer claims \
that are well supported. Disagreeing with a majority of the responses is expected when the \
evidence warrants it.";

/// Additional clause appended when `single_source_cap` is true: prevents any one panel
/// response from dominating the judge's or synthesizer's output.
const HARDENING_SINGLE_SOURCE: &str = "Do not let any single response dominate your answer.";

/// Build the hardening suffix: base text always, dominance clause when `single_source_cap`.
fn hardening(single_source_cap: bool) -> String {
    if single_source_cap {
        format!("{HARDENING_BASE} {HARDENING_SINGLE_SOURCE}")
    } else {
        HARDENING_BASE.to_string()
    }
}

/// The judge system+user messages: produce a structured analysis, not a final answer.
///
/// When `single_source_cap` is true, the system prompt includes an instruction that no
/// single panel response should dominate the analysis.
///
/// # Panics
///
/// Does not panic; `format!` is infallible here.
#[must_use]
pub fn judge_messages(query: &str, references: &str, single_source_cap: bool) -> Vec<ChatMessage> {
    let h = hardening(single_source_cap);
    let system = format!(
        "You are a careful analyst. You are given a user query and several anonymized candidate \
responses. {h} Produce a STRUCTURED ANALYSIS with these sections: Consensus, \
Contradictions, Unique insights, Blind spots. Do not write a final answer."
    );
    let user = format!("User query:\n{query}\n\nCandidate responses:\n{references}");
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// The synthesis system+user messages: write the final grounded answer.
///
/// When `single_source_cap` is true, the system prompt includes an instruction that no
/// single panel response should dominate the final answer.
///
/// # Panics
///
/// Does not panic; `format!` is infallible here.
#[must_use]
pub fn synthesis_messages(
    query: &str,
    references: &str,
    analysis: &str,
    single_source_cap: bool,
) -> Vec<ChatMessage> {
    let h = hardening(single_source_cap);
    let system = format!(
        "You are a synthesizer. Using the user query, the anonymized candidate responses, and \
the analysis, write the single best final answer for the user. {h} Write the answer \
directly, with no meta commentary about the responses or the analysis."
    );
    let user = format!(
        "User query:\n{query}\n\nCandidate responses:\n{references}\n\nAnalysis:\n{analysis}"
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// The refine messages for a multi-layer panel member: given the user query and
/// the previous layer's anonymized candidate responses, write an improved
/// standalone answer (not an analysis). The same hardening as the judge and
/// synthesizer applies, so a member does not simply replicate or average the
/// prior layer. Used only when `aggregator.layers > 1` (issue #20).
///
/// # Panics
///
/// Does not panic; `format!` is infallible here.
#[must_use]
pub fn refine_messages(query: &str, references: &str, single_source_cap: bool) -> Vec<ChatMessage> {
    let h = hardening(single_source_cap);
    let system = format!(
        "You are answering a user query. You are also given several anonymized candidate \
responses from other models for the same query. {h} Use them only to improve your own answer: \
correct errors, fill gaps, and add support. Write a single improved standalone answer to the \
query, directly, with no meta commentary about the other responses."
    );
    let user = format!("User query:\n{query}\n\nCandidate responses:\n{references}");
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// Messages that ask a cheap model to rate query difficulty as a single number.
#[must_use]
pub fn difficulty_messages(query: &str) -> Vec<ChatMessage> {
    let system = "You are a routing classifier. Rate how hard the user query is to answer \
well, as a single decimal number between 0.0 (trivial, a single capable model answers it \
perfectly) and 1.0 (very hard, benefits from multiple models and synthesis). Reply with ONLY \
the number, nothing else.";
    vec![
        ChatMessage::system(system),
        ChatMessage::user(format!("Query:\n{query}")),
    ]
}

/// Parse a difficulty score in 0.0..=1.0 from a model reply, tolerating surrounding text.
/// Returns None if no parseable number is found.
#[must_use]
pub fn parse_difficulty(text: &str) -> Option<f32> {
    // Find the first run that parses as a float; clamp to 0.0..=1.0.
    let mut buf = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
        } else if !buf.is_empty() {
            break;
        }
    }
    buf.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod difficulty_tests {
    use super::{difficulty_messages, parse_difficulty};

    #[test]
    fn difficulty_prompt_asks_for_a_single_number() {
        let msgs = difficulty_messages("q");
        assert!(msgs[0].content.contains("single"));
        assert!(msgs[0].content.contains("ONLY the number"));
    }

    #[test]
    fn parses_bare_and_noisy_scores() {
        assert_eq!(parse_difficulty("0.8"), Some(0.8));
        assert_eq!(parse_difficulty("The difficulty is 0.3 overall"), Some(0.3));
        assert_eq!(parse_difficulty("1"), Some(1.0));
        assert_eq!(parse_difficulty("nonsense"), None);
    }

    #[test]
    fn clamps_out_of_range() {
        assert_eq!(parse_difficulty("2.5"), Some(1.0));
    }
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
        let msgs = judge_messages("q", "refs", true);
        let sys = &msgs[0].content;
        assert!(sys.contains("STRUCTURED ANALYSIS"));
        assert!(sys.contains("Do not write a final answer"));
        assert!(sys.contains("Do not let any single response dominate"));
        // base hardening is always present
        assert!(sys.contains("biased, incorrect"));
    }

    #[test]
    fn judge_prompt_omits_dominance_clause_when_flag_off() {
        let msgs = judge_messages("q", "refs", false);
        let sys = &msgs[0].content;
        // base hardening still present
        assert!(sys.contains("biased, incorrect"));
        // dominance clause must be absent
        assert!(!sys.contains("Do not let any single response dominate"));
    }

    // ---------------------------------------------------------------------------
    // Test 4: label_for wraps at 26 (A..Z then back to A)
    // ---------------------------------------------------------------------------
    #[test]
    fn label_for_wraps_after_z() {
        assert_eq!(label_for(0), 'A', "slot 0 must be 'A'");
        assert_eq!(label_for(25), 'Z', "slot 25 must be 'Z'");
        // slot 26 must wrap back to 'A'
        assert_eq!(label_for(26), 'A', "slot 26 must wrap to 'A'");
    }

    /// Verify the wrap via `format_references` with 27 responses.
    /// The 27th response (index 26) is labelled "Response A" again because
    /// `label_for` wraps at 26. This is a design limitation documented here:
    /// panels with more than 26 members will produce duplicate labels.
    #[test]
    fn format_references_27th_label_wraps_to_a() {
        let responses: Vec<String> = (0..27).map(|i| format!("answer{i}")).collect();
        let formatted = format_references(&responses, false, 1_000);
        // The 27th entry (index 26) must carry label A again.
        // Because entry 0 is also label A, we count occurrences of "Response A:" to confirm
        // the wrap produces a duplicate -- both slots 0 and 26 bear that label.
        let count = formatted.matches("Response A:").count();
        assert_eq!(
            count, 2,
            "expected exactly 2 'Response A:' entries (slots 0 and 26), got {count}"
        );
    }

    #[test]
    fn refine_prompt_asks_for_an_answer_with_hardening() {
        let msgs = refine_messages("q", "refs", true);
        let sys = &msgs[0].content;
        // It must ask for an improved standalone answer, not an analysis.
        assert!(sys.contains("improved standalone answer"));
        assert!(!sys.contains("STRUCTURED ANALYSIS"));
        // Hardening is carried, including the dominance clause when capped.
        assert!(sys.contains("biased, incorrect"));
        assert!(sys.contains("Do not let any single response dominate"));
        // The user message carries the query and the references.
        assert!(msgs[1].content.contains("User query:"));
        assert!(msgs[1].content.contains("refs"));
    }

    #[test]
    fn synthesis_prompt_carries_hardening() {
        let msgs = synthesis_messages("q", "refs", "analysis", true);
        assert!(
            msgs[0]
                .content
                .contains("Do not let any single response dominate")
        );
        assert!(msgs[0].content.contains("biased, incorrect"));
    }
}
