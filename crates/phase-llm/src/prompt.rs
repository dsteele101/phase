//! The prompt pair handed to a provider, the difficulty persona that shapes it,
//! and the decoder that turns a free-text reply back into engine option indices.

use phase_ai::config::AiDifficulty;
use serde::Serialize;
use serde_json::Value;

use crate::error::{LlmError, LlmResult};

/// A system/user message pair. Every protocol in [`crate::wire`] carries these
/// two fields, whatever it calls them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmPrompt {
    pub system: String,
    pub user: String,
}

/// What the model was asked to do, decoded back out of its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmChoice {
    /// Engine option indices, in the order the model named them, deduplicated
    /// and already bounds-checked against the offered option count.
    pub indices: Vec<usize>,
    /// The model's own one-line justification, when it supplied one. Surfaced
    /// in local diagnostics; never fed back into the engine.
    pub reasoning: Option<String>,
}

/// How hard the LLM opponent is asked to play.
///
/// Difficulty is not decoration here: it is the single lever that makes an LLM
/// seat comparable to a heuristic seat at the same setting, so it shapes the
/// persona, the strategic instruction, and how much of the board the model is
/// asked to reason over.
pub fn difficulty_brief(difficulty: AiDifficulty) -> &'static str {
    match difficulty {
        AiDifficulty::VeryEasy => {
            "You are a brand-new player who has just learned the rules. Play the \
             first reasonable-looking option. Do not plan ahead, do not count \
             your opponent's open mana, and do not hold cards back for later \
             turns. Making a clearly suboptimal but legal play is expected and \
             correct at this level."
        }
        AiDifficulty::Easy => {
            "You are a casual kitchen-table player. Play your cards roughly on \
             curve and attack when it looks safe, but do not calculate exact \
             combat math, do not play around cards your opponent might be \
             holding, and do not build multi-turn plans."
        }
        AiDifficulty::Medium => {
            "You are a solid regular player. Develop your board on curve, make \
             favourable trades, count lethal damage, and hold up interaction \
             when it is cheap to do so. Play around the most obvious cards your \
             opponent could have, but do not agonise over unlikely lines."
        }
        AiDifficulty::Hard => {
            "You are a strong competitive player. Sequence your plays to \
             maximise mana efficiency, count exact combat and racing math, \
             track what your opponent has shown and what their open mana \
             represents, and pick the line that wins fastest while losing to \
             the fewest outs."
        }
        AiDifficulty::VeryHard => {
            "You are an expert tournament player. Evaluate every legal option \
             for its effect on the whole game, not just this turn. Count exact \
             damage, track every card revealed so far, infer your opponent's \
             hand from their mana and play pattern, and deliberately play \
             around the specific cards that beat you. Never make a play that is \
             merely 'fine' when a better one exists."
        }
        AiDifficulty::CEDH => {
            "You are a competitive Commander (cEDH) specialist. Assume every \
             opponent is playing a tuned, fast combo deck. Prioritise \
             assembling or protecting your own win condition, holding \
             interaction for opposing combo attempts, and denying the fastest \
             opponent. Value card selection, fast mana, and stack interaction \
             far above incremental board presence."
        }
    }
}

/// Whether this difficulty should be shown the full move history and the full
/// public board, or only the immediate position.
///
/// Returned as a line count rather than a bool so the caller has the actual
/// budget: the two lowest difficulties deliberately reason from a near-term
/// window, which is a large part of what makes them beatable.
pub fn history_window(difficulty: AiDifficulty) -> usize {
    match difficulty {
        AiDifficulty::VeryEasy => 0,
        AiDifficulty::Easy => 10,
        AiDifficulty::Medium => 30,
        AiDifficulty::Hard => 60,
        AiDifficulty::VeryHard | AiDifficulty::CEDH => 100,
    }
}

/// The reply contract, appended to every decision prompt. Kept in one constant
/// because [`decode_choice`] is written against exactly this shape.
pub const RESPONSE_CONTRACT: &str =
    "Reply with ONLY a JSON object and nothing else, in this form:\n\
     {\"choice\": <the number of the option you pick>, \"reason\": \"<one short sentence>\"}\n\
     Do not wrap it in markdown. Do not explain outside the JSON. The \"choice\" \
     value must be one of the option numbers listed above.";

/// The multi-pick variant of [`RESPONSE_CONTRACT`], for a draft step that takes
/// more than one card (CR 903.13b).
pub fn multi_response_contract(required: usize) -> String {
    format!(
        "Reply with ONLY a JSON object and nothing else, in this form:\n\
         {{\"choice\": [<{required} option numbers, best first>], \"reason\": \"<one short sentence>\"}}\n\
         Do not wrap it in markdown. Do not explain outside the JSON. Every value \
         must be one of the option numbers listed above, and they must be distinct."
    )
}

/// Decode a model's reply into option indices.
///
/// STRICT by construction. The only accepted shapes are a JSON object whose
/// choice field holds a number, an exact integer string, or an array of those;
/// and a reply whose entire trimmed text is a single integer. Fenced JSON and
/// prose surrounding a JSON object are tolerated because neither changes which
/// number is the answer.
///
/// What is NOT accepted is any reply where the answer must be *inferred* from
/// prose. "I considered option 2, but choose 3" contains two integers and no
/// structural rule picks the right one — scanning would silently select 2, a
/// legal action the model did not choose. A wrong-but-legal action is the worst
/// possible outcome here: it is invisible, it is attributed to the model, and
/// it changes the game. Refusing costs one heuristic fallback, so every
/// ambiguity resolves to a refusal.
///
/// An index outside the offered domain is likewise an error, never a clamp.
pub fn decode_choice(text: &str, option_count: usize, wanted: usize) -> LlmResult<LlmChoice> {
    if option_count == 0 {
        return Err(LlmError::UndecodableChoice {
            detail: "no options were offered".to_string(),
        });
    }

    let stripped = strip_code_fences(text);
    let object = extract_json_object(stripped);

    let reasoning = object
        .as_ref()
        .and_then(reasoning_field)
        .map(str::to_string);

    let raw = object
        .as_ref()
        .and_then(choice_field)
        .map(collect_numbers)
        .transpose()?
        .filter(|numbers| !numbers.is_empty())
        .or_else(|| {
            // No usable JSON. The one unambiguous non-JSON reply is a bare
            // integer and nothing else — not a sentence that happens to contain
            // one, which is where a scan would start guessing.
            parse_exact_integer(stripped).map(|number| vec![number])
        })
        .ok_or_else(|| LlmError::UndecodableChoice {
            detail: format!(
                "reply is not a structured choice: {:?}",
                truncate(text, 200)
            ),
        })?;

    let mut indices: Vec<usize> = Vec::with_capacity(wanted.max(1));
    for number in raw {
        let index = usize::try_from(number).map_err(|_| LlmError::ChoiceOutOfRange {
            choice: number,
            option_count,
        })?;
        if index >= option_count {
            return Err(LlmError::ChoiceOutOfRange {
                choice: number,
                option_count,
            });
        }
        if !indices.contains(&index) {
            indices.push(index);
        }
        if indices.len() == wanted.max(1) {
            break;
        }
    }

    if indices.is_empty() {
        return Err(LlmError::UndecodableChoice {
            detail: "reply named no distinct in-range option".to_string(),
        });
    }

    Ok(LlmChoice { indices, reasoning })
}

/// Remove a surrounding ```/```json fence, which models add despite being asked
/// not to. Returns the original text when there is no fence to strip.
fn strip_code_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    // The opening fence may carry a language tag; the body starts at the newline.
    let body = after_open
        .split_once('\n')
        .map_or(after_open, |(_tag, rest)| rest);
    body.rsplit_once("```")
        .map_or(body, |(inner, _)| inner)
        .trim()
}

/// The first balanced `{...}` span that parses as JSON. Scans rather than
/// requiring the whole reply to be JSON so prose around the object is harmless.
fn extract_json_object(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    for (start, _) in text.char_indices().filter(|(_, c)| *c == '{') {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in bytes[start..].iter().enumerate() {
            if in_string {
                match byte {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'"' => in_string = false,
                    _ => {}
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + offset + 1;
                        if let Ok(value) = serde_json::from_str::<Value>(&text[start..end]) {
                            return Some(value);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The keys a model plausibly uses for its answer, in preference order.
fn choice_field(object: &Value) -> Option<&Value> {
    [
        "choice", "choices", "option", "options", "index", "pick", "picks",
    ]
    .iter()
    .find_map(|key| object.get(key))
}

fn reasoning_field(object: &Value) -> Option<&str> {
    ["reason", "reasoning", "why", "explanation"]
        .iter()
        .find_map(|key| object.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
}

/// Numbers carried by a `choice` field.
///
/// Accepts a JSON number, a string that is EXACTLY an integer, or an array of
/// those. Any other shape — a sentence, a float, a nested object — is an error
/// rather than a salvage attempt, so a field the model filled with prose can
/// never resolve to one of the numbers inside that prose.
fn collect_numbers(value: &Value) -> LlmResult<Vec<i64>> {
    match value {
        Value::Number(number) => {
            number
                .as_i64()
                .map(|number| vec![number])
                .ok_or_else(|| LlmError::UndecodableChoice {
                    detail: format!("choice {number} is not a whole number"),
                })
        }
        Value::String(text) => parse_exact_integer(text)
            .map(|number| vec![number])
            .ok_or_else(|| LlmError::UndecodableChoice {
                detail: format!(
                    "choice {:?} is not a bare option number",
                    truncate(text, 80)
                ),
            }),
        Value::Array(items) => {
            let mut numbers = Vec::with_capacity(items.len());
            for item in items {
                numbers.extend(collect_numbers(item)?);
            }
            Ok(numbers)
        }
        other => Err(LlmError::UndecodableChoice {
            detail: format!("choice field is {other}, not an option number"),
        }),
    }
}

/// `Some(n)` only when `text` is entirely one non-negative integer, ignoring
/// surrounding whitespace and a single trailing period. Nothing else parses:
/// no embedded number, no sign, no decimal point.
fn parse_exact_integer(text: &str) -> Option<i64> {
    let trimmed = text.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<i64>().ok()
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_json_reply_decodes() {
        let choice = decode_choice(r#"{"choice": 2, "reason": "best blocker"}"#, 5, 1).unwrap();
        assert_eq!(choice.indices, vec![2]);
        assert_eq!(choice.reasoning.as_deref(), Some("best blocker"));
    }

    #[test]
    fn a_fenced_json_reply_decodes() {
        let choice = decode_choice("```json\n{\"choice\": 0}\n```", 3, 1).unwrap();
        assert_eq!(choice.indices, vec![0]);
    }

    #[test]
    fn prose_around_the_object_is_ignored() {
        let choice = decode_choice(
            "Thinking about it...\n{\"choice\": 1}\nHope that helps!",
            4,
            1,
        )
        .unwrap();
        assert_eq!(choice.indices, vec![1]);
    }

    #[test]
    fn a_bare_integer_reply_decodes() {
        assert_eq!(decode_choice("3", 5, 1).unwrap().indices, vec![3]);
        // Whitespace and a single trailing period are punctuation, not content.
        assert_eq!(decode_choice("  3.  ", 5, 1).unwrap().indices, vec![3]);
    }

    #[test]
    fn a_multi_pick_reply_keeps_order_and_drops_repeats() {
        let choice = decode_choice(r#"{"choice": [4, 4, 1]}"#, 6, 2).unwrap();
        assert_eq!(choice.indices, vec![4, 1]);
    }

    #[test]
    fn an_out_of_range_choice_is_an_error_not_a_clamp() {
        assert_eq!(
            decode_choice(r#"{"choice": 9}"#, 3, 1),
            Err(LlmError::ChoiceOutOfRange {
                choice: 9,
                option_count: 3
            })
        );
    }

    #[test]
    fn a_reply_with_no_number_is_undecodable() {
        assert!(matches!(
            decode_choice("I am not sure what to do here.", 3, 1),
            Err(LlmError::UndecodableChoice { .. })
        ));
    }

    /// The finding this strictness exists for: a reply naming two options in
    /// prose has no structural answer, and scanning selected the FIRST one — a
    /// legal action the model had explicitly rejected.
    #[test]
    fn prose_naming_two_options_is_refused_rather_than_resolved_to_the_first() {
        for reply in [
            "I considered option 2, but choose 3",
            "Not 2 — go with 3.",
            "Between 2 and 3 I prefer 3",
        ] {
            assert!(
                matches!(
                    decode_choice(reply, 5, 1),
                    Err(LlmError::UndecodableChoice { .. })
                ),
                "must refuse: {reply}"
            );
        }
    }

    /// Even a sentence a reader finds unambiguous is refused: accepting it
    /// re-introduces scanning, and the reader's confidence does not generalize.
    #[test]
    fn a_single_number_embedded_in_prose_is_still_refused() {
        assert!(matches!(
            decode_choice("I'll take option 2.", 5, 1),
            Err(LlmError::UndecodableChoice { .. })
        ));
    }

    #[test]
    fn a_choice_string_carrying_prose_is_refused() {
        for reply in [
            r#"{"choice": "option 2 or maybe 3"}"#,
            r#"{"choice": "I pick 2"}"#,
            r#"{"choice": "two"}"#,
        ] {
            assert!(
                matches!(
                    decode_choice(reply, 5, 1),
                    Err(LlmError::UndecodableChoice { .. })
                ),
                "must refuse: {reply}"
            );
        }
    }

    #[test]
    fn a_non_numeric_choice_field_is_refused() {
        for reply in [
            r#"{"choice": null}"#,
            r#"{"choice": true}"#,
            r#"{"choice": 1.5}"#,
            r#"{"choice": {"index": 1}}"#,
        ] {
            assert!(
                matches!(
                    decode_choice(reply, 5, 1),
                    Err(LlmError::UndecodableChoice { .. })
                ),
                "must refuse: {reply}"
            );
        }
    }

    /// One bad entry refuses the whole selection rather than yielding a shorter
    /// list, which a multi-pick step would treat as a partial answer.
    #[test]
    fn a_mixed_array_is_refused_whole() {
        assert!(matches!(
            decode_choice(r#"{"choice": [1, "pick 2"]}"#, 5, 2),
            Err(LlmError::UndecodableChoice { .. })
        ));
    }

    /// Prose around a structural answer stays harmless — the JSON object
    /// decides — so card stats and confidences contribute no number.
    #[test]
    fn prose_around_a_json_object_never_contributes_a_number() {
        assert_eq!(
            decode_choice("The 2/2 trades with their 3/3. {\"choice\": 1}", 4, 1)
                .unwrap()
                .indices,
            vec![1]
        );
        assert_eq!(
            decode_choice("confidence 0.85 — {\"choice\": 1}", 4, 1)
                .unwrap()
                .indices,
            vec![1]
        );
    }

    #[test]
    fn a_numeric_string_choice_decodes() {
        assert_eq!(
            decode_choice(r#"{"choice": "2"}"#, 4, 1).unwrap().indices,
            vec![2]
        );
    }

    #[test]
    fn an_empty_option_domain_never_decodes() {
        assert!(matches!(
            decode_choice(r#"{"choice": 0}"#, 0, 1),
            Err(LlmError::UndecodableChoice { .. })
        ));
    }

    #[test]
    fn every_difficulty_has_a_distinct_brief_and_a_history_window() {
        let difficulties = [
            AiDifficulty::VeryEasy,
            AiDifficulty::Easy,
            AiDifficulty::Medium,
            AiDifficulty::Hard,
            AiDifficulty::VeryHard,
            AiDifficulty::CEDH,
        ];
        let mut briefs: Vec<&str> = difficulties.iter().copied().map(difficulty_brief).collect();
        briefs.sort_unstable();
        briefs.dedup();
        assert_eq!(briefs.len(), difficulties.len());
        // Monotonic: a harder seat never sees less history than an easier one.
        for pair in difficulties.windows(2) {
            assert!(history_window(pair[0]) <= history_window(pair[1]));
        }
    }
}
