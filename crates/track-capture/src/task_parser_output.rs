use track_core::errors::{ErrorCode, TrackError};
use track_core::types::ParsedTaskCandidate;

// =============================================================================
// Local Model Output Recovery
// =============================================================================
//
// Both local parser backends ultimately need the same last-mile behavior:
// extract one `ParsedTaskCandidate` from output that may contain prompt echo,
// logging noise, or terminal metadata around the actual JSON payload.
//
// Keeping that recovery logic in one module prevents the subprocess parser and
// the in-process parser from drifting apart in subtle ways when we tighten the
// task schema or improve JSON extraction.

pub fn combine_model_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);

    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

pub fn parse_candidate_from_command_output(
    stdout: &[u8],
    stderr: &[u8],
) -> Result<ParsedTaskCandidate, TrackError> {
    let stdout_text = String::from_utf8_lossy(stdout).into_owned();
    if let Ok(candidate) = parse_candidate_from_model_output(&stdout_text) {
        return Ok(candidate);
    }

    let stderr_text = String::from_utf8_lossy(stderr).into_owned();
    if stdout_text.trim().is_empty() {
        if let Ok(candidate) = parse_candidate_from_model_output(&stderr_text) {
            return Ok(candidate);
        }
    }

    let combined_output = combine_model_output(stdout, stderr);
    parse_candidate_from_model_output(&combined_output)
}

pub fn parse_candidate_from_model_output(output: &str) -> Result<ParsedTaskCandidate, TrackError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. The local model returned an empty response.",
        ));
    }

    // Some llama.cpp builds still echo prompts or print terminal metadata
    // around the actual answer. Rather than grabbing the first `{` and hoping
    // for the best, we walk the whole transcript, collect balanced JSON
    // objects, and accept the last one that actually matches the task schema.
    let mut saw_json_object = false;
    let mut last_error = None;

    for candidate in json_object_candidates(trimmed).into_iter().rev() {
        saw_json_object = true;

        match serde_json::from_str::<ParsedTaskCandidate>(candidate) {
            Ok(parsed) => return Ok(parsed),
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(error) = last_error {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            format!("AI parse failure. The local model did not return valid task JSON: {error}"),
        ));
    }

    if saw_json_object {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. The local model did not return valid task JSON.",
        ));
    }

    Err(TrackError::new(
        ErrorCode::AiParseFailed,
        "AI parse failure. The local model did not return valid JSON.",
    ))
}

fn json_object_candidates(output: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in output.char_indices() {
        if in_string {
            match character {
                '"' if !escaped => {
                    in_string = false;
                    escaped = false;
                }
                '\\' if !escaped => escaped = true,
                _ => escaped = false,
            }

            continue;
        }

        match character {
            '"' => {
                in_string = true;
                escaped = false;
            }
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }

                depth -= 1;
                if depth == 0 {
                    if let Some(start_index) = start.take() {
                        let end_index = index + character.len_utf8();
                        candidates.push(&output[start_index..end_index]);
                    }
                }
            }
            _ => {}
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use track_core::types::{Confidence, ParsedTaskCandidate, Priority};

    use super::{
        combine_model_output, json_object_candidates, parse_candidate_from_command_output,
        parse_candidate_from_model_output,
    };

    #[test]
    fn combines_stdout_and_stderr_when_both_have_content() {
        let combined = combine_model_output(b"stdout line\n", b"stderr line\n");

        assert_eq!(combined, "stdout line\n\nstderr line\n");
    }

    #[test]
    fn parses_candidate_from_chatty_terminal_output() {
        let output = r#"
Loading model...

> {
  "rawText": "airbender prio high resolve clippy warnings",
  "allowedProjects": [
    {
      "canonicalName": "zksync-airbender",
      "aliases": [
        "airbender"
      ]
    }
  ],
  "expectedJsonShape": {
    "project": "project-name-or-alias-or-null",
    "priority": "high|medium|low",
    "title": "Concise actionable sentence",
    "bodyMarkdown": "Optional supporting markdown, without repeating the title",
    "confidence": "high|low",
    "reason": "Optional short explanation"
  }
}

{
  "bodyMarkdown": "- Run `cargo clippy --workspace` and fix the reported warnings.",
  "confidence": "high",
  "priority": "high",
  "project": "airbender",
  "title": "Resolve clippy warnings",
  "reason": null
}

[ Prompt: 89.5 t/s | Generation: 12.2 t/s ]
Exiting...
"#;

        let candidate =
            parse_candidate_from_model_output(output).expect("candidate should parse from output");

        assert_eq!(
            candidate,
            ParsedTaskCandidate {
                project: Some("airbender".to_owned()),
                priority: Priority::High,
                title: "Resolve clippy warnings".to_owned(),
                body_markdown: Some(
                    "- Run `cargo clippy --workspace` and fix the reported warnings.".to_owned(),
                ),
                confidence: Confidence::High,
                reason: None,
            }
        );
    }

    #[test]
    fn keeps_balanced_json_objects_separate_when_prompt_echo_contains_json() {
        let output = r#"prefix {"rawText":"example","expectedJsonShape":{"project":"name"}} suffix {"project":"project-x","priority":"high","title":"Fix a bug","bodyMarkdown":"","confidence":"high"}"#;
        let candidates = json_object_candidates(output);

        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[1],
            r#"{"project":"project-x","priority":"high","title":"Fix a bug","bodyMarkdown":"","confidence":"high"}"#
        );
    }

    #[test]
    fn prefers_clean_stdout_over_noisy_stderr() {
        let candidate = parse_candidate_from_command_output(
            br#"{"project":"project-x","priority":"high","title":"Fix a bug","bodyMarkdown":"","confidence":"high"}"#,
            b"memory stats...\n",
        )
        .expect("candidate should parse from stdout");

        assert_eq!(
            candidate,
            ParsedTaskCandidate {
                project: Some("project-x".to_owned()),
                priority: Priority::High,
                title: "Fix a bug".to_owned(),
                body_markdown: Some(String::new()),
                confidence: Confidence::High,
                reason: None,
            }
        );
    }
}
