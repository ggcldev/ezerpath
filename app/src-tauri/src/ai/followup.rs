// Local resolver for follow-up messages that reference prior results
// ("the first one", "those two", "all of them", etc.).
//
// When confident, this lets us short-circuit the Ollama JSON-mode path —
// no HTTP round-trip, no parse failure modes, no prompt drift. When not
// confident, the caller falls through to `chat_json` untouched, so this
// is pure upside.
//
// The resolver stays strictly *selection-and-summary*: it picks which prior
// job IDs the user meant, and optionally asks for the stored summaries to
// be returned verbatim. It never attempts generative tasks like
// "compare these two" or "which pays more" — those still go to the LLM.

use std::collections::HashSet;

/// The action the local resolver decided on. `None` from the public entry
/// point means "I'm not confident — let the LLM handle it."
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpAction {
    /// User just wants to re-view a subset of the prior cards.
    Select(Vec<i64>),
    /// User wants the stored `summary` field for a subset of the prior
    /// jobs, which we can fulfill directly from SQL with zero LLM cost.
    Describe(Vec<i64>),
}

const MAX_REFERENTIAL_WORDS: usize = 18;

/// Single-token generative triggers — matched against word-boundary
/// tokens only so "show" doesn't trip "how", "shy" doesn't trip "why", etc.
const GENERATIVE_TOKENS: &[&str] = &[
    "compare", "vs", "versus", "pros", "cons", "better", "worse",
    "recommend", "suggest", "why", "how",
];

/// Multi-word generative phrases — matched as substrings (still rare
/// enough to be safe).
const GENERATIVE_PHRASES: &[&str] = &["should i"];

/// Single-token describe triggers.
const DESCRIBE_TOKENS: &[&str] = &[
    "describe", "description", "details", "summary", "summarize",
];

/// Multi-word describe phrases.
const DESCRIBE_PHRASES: &[&str] = &[
    "tell me about", "what do they do", "what does it do", "what's it about",
    "what are they", "what is it",
];

fn tokenize(lower: &str) -> HashSet<String> {
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn contains_any_token(tokens: &HashSet<String>, candidates: &[&str]) -> bool {
    candidates.iter().any(|t| tokens.contains(*t))
}

fn contains_any_phrase(lower: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|p| lower.contains(*p))
}

/// Pure entry point. Returns `None` if the message is ambiguous or clearly
/// needs LLM-level understanding.
pub fn resolve_followup(message: &str, prev_ids: &[i64]) -> Option<FollowUpAction> {
    if prev_ids.is_empty() {
        return None;
    }
    let lower = message.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let word_count = lower.split_whitespace().count();
    if word_count > MAX_REFERENTIAL_WORDS {
        return None;
    }

    let tokens = tokenize(&lower);
    let wants_description =
        contains_any_token(&tokens, DESCRIBE_TOKENS) || contains_any_phrase(&lower, DESCRIBE_PHRASES);

    // Anything comparative or generative is out of scope — even if the
    // user also asked for a description. The LLM picks the ordering and
    // phrasing better than we can.
    if contains_any_token(&tokens, GENERATIVE_TOKENS)
        || contains_any_phrase(&lower, GENERATIVE_PHRASES)
    {
        return None;
    }

    let selection = pick_selection(&lower, &tokens, prev_ids)?;
    if wants_description {
        Some(FollowUpAction::Describe(selection))
    } else {
        Some(FollowUpAction::Select(selection))
    }
}

/// Decide which subset of `prev_ids` the message refers to. Returns
/// `None` when nothing in the message constitutes an actual reference
/// — that's how we tell "show me the first two" (fast path) from
/// "find me rust jobs" (new search).
fn pick_selection(
    lower: &str,
    tokens: &HashSet<String>,
    prev_ids: &[i64],
) -> Option<Vec<i64>> {
    // Ordinal single-pick: "the third one", "second one", "3rd one".
    if let Some(idx) = parse_single_ordinal(lower) {
        if idx >= 1 && idx <= prev_ids.len() {
            return Some(vec![prev_ids[idx - 1]]);
        }
        return None;
    }

    // "last one" / "the last"
    if lower.contains("last one") || lower.contains("the last") {
        if let Some(last) = prev_ids.last().copied() {
            return Some(vec![last]);
        }
    }

    // Prefix count: "first two", "top 3", "those three", "first 5".
    if let Some(n) = parse_prefix_count(lower) {
        let take = n.min(prev_ids.len());
        if take > 0 {
            return Some(prev_ids[..take].to_vec());
        }
    }

    // Explicit select-all phrases.
    const ALL_PHRASES: &[&str] = &[
        "all of them", "all of these", "all of those",
        "every one of them", "each of them", "each one",
    ];
    if ALL_PHRASES.iter().any(|p| lower.contains(p)) {
        return Some(prev_ids.to_vec());
    }

    // Bare pronoun selection: "those", "them", "these" as full tokens.
    // This is the default when the user refers to the prior list without
    // narrowing it at all.
    const PRONOUNS: &[&str] = &["those", "them", "these"];
    if contains_any_token(tokens, PRONOUNS) {
        return Some(prev_ids.to_vec());
    }

    None
}

/// Parses phrases like "the third one", "third one", "3rd one", "item 2".
/// Returns 1-based index.
fn parse_single_ordinal(lower: &str) -> Option<usize> {
    const WORDS: &[(&str, usize)] = &[
        ("first one", 1), ("second one", 2), ("third one", 3),
        ("fourth one", 4), ("fifth one", 5), ("sixth one", 6),
        ("seventh one", 7), ("eighth one", 8), ("ninth one", 9),
        ("tenth one", 10),
        ("1st one", 1), ("2nd one", 2), ("3rd one", 3), ("4th one", 4),
        ("5th one", 5), ("6th one", 6), ("7th one", 7), ("8th one", 8),
        ("9th one", 9), ("10th one", 10),
    ];
    for (needle, idx) in WORDS {
        if lower.contains(needle) {
            return Some(*idx);
        }
    }
    // "the second", "the third" without "one" — still unambiguous if
    // followed by end-of-message or a noun like "job" / "role".
    const BARE_ORDINALS: &[(&str, usize)] = &[
        ("the first", 1), ("the second", 2), ("the third", 3),
        ("the fourth", 4), ("the fifth", 5),
    ];
    for (needle, idx) in BARE_ORDINALS {
        if lower.contains(needle) {
            // Reject if it looks like "the first two / three" — that's a
            // prefix count, handled elsewhere.
            let after = lower.split(needle).nth(1).unwrap_or("").trim_start();
            let next = after.split_whitespace().next().unwrap_or("");
            if matches!(next, "two" | "three" | "four" | "five" | "2" | "3" | "4" | "5") {
                continue;
            }
            return Some(*idx);
        }
    }
    None
}

/// Parses "first two", "top 3", "those three", "first 5 jobs", etc.
fn parse_prefix_count(lower: &str) -> Option<usize> {
    const NUM_WORDS: &[(&str, usize)] = &[
        ("two", 2), ("three", 3), ("four", 4), ("five", 5),
        ("six", 6), ("seven", 7), ("eight", 8), ("nine", 9), ("ten", 10),
    ];
    const TRIGGERS: &[&str] = &["first", "top", "those", "these"];
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let trigger_set: HashSet<&str> = TRIGGERS.iter().copied().collect();

    for i in 0..tokens.len().saturating_sub(1) {
        if !trigger_set.contains(tokens[i]) {
            continue;
        }
        let next = tokens[i + 1].trim_matches(|c: char| !c.is_alphanumeric());
        if let Ok(n) = next.parse::<usize>() {
            if (2..=20).contains(&n) {
                return Some(n);
            }
        }
        if let Some((_, n)) = NUM_WORDS.iter().find(|(w, _)| *w == next) {
            return Some(*n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> Vec<i64> {
        vec![101, 102, 103, 104, 105]
    }

    #[test]
    fn resolves_single_ordinal() {
        let out = resolve_followup("show me the second one", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(vec![102])));
    }

    #[test]
    fn resolves_numeric_ordinal_suffix() {
        let out = resolve_followup("just the 3rd one", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(vec![103])));
    }

    #[test]
    fn resolves_prefix_count_word() {
        let out = resolve_followup("show me the first two", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(vec![101, 102])));
    }

    #[test]
    fn resolves_prefix_count_digit() {
        let out = resolve_followup("just those 3", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(vec![101, 102, 103])));
    }

    #[test]
    fn resolves_last() {
        let out = resolve_followup("the last one please", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(vec![105])));
    }

    #[test]
    fn resolves_select_all() {
        let out = resolve_followup("show me all of them", &ids());
        assert_eq!(out, Some(FollowUpAction::Select(ids())));
    }

    #[test]
    fn routes_describe_when_summary_word_present() {
        let out = resolve_followup("describe the first two", &ids());
        assert_eq!(out, Some(FollowUpAction::Describe(vec![101, 102])));
    }

    #[test]
    fn bails_on_comparative_language() {
        let out = resolve_followup("compare the first two", &ids());
        assert_eq!(out, None);
    }

    #[test]
    fn bails_on_why_questions() {
        let out = resolve_followup("why is the first one better", &ids());
        assert_eq!(out, None);
    }

    #[test]
    fn bails_without_reference_anchor() {
        let out = resolve_followup("find me rust jobs", &ids());
        assert_eq!(out, None);
    }

    #[test]
    fn bails_on_long_messages() {
        let msg = "i have been thinking a lot about this and i want to know which of the first two actually have benefits included for remote workers in asia";
        let out = resolve_followup(msg, &ids());
        assert_eq!(out, None);
    }

    #[test]
    fn bails_on_empty_prev_ids() {
        let out = resolve_followup("the second one", &[]);
        assert_eq!(out, None);
    }

    #[test]
    fn ordinal_out_of_range_bails() {
        let out = resolve_followup("show me the ninth one", &ids());
        assert_eq!(out, None);
    }
}
