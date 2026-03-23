use std::env;
use std::process::Command;

use serde_json::json;

use crate::errors::{ErrorCode, TrackError};
use crate::paths::path_to_string;
use crate::project_catalog::ProjectCatalog;
use crate::prompt::{build_llama_cpp_prompt, DEFAULT_LLAMA_CPP_COMPLETION_BINARY};
use crate::time_utils::format_iso_8601_millis;
use crate::types::ParsedTaskCandidate;

pub struct LlamaCppTaskParser {
    binary_path: String,
    model_path: std::path::PathBuf,
}

impl LlamaCppTaskParser {
    pub fn new(binary_path: Option<String>, model_path: std::path::PathBuf) -> Self {
        Self {
            binary_path: binary_path
                .unwrap_or_else(|| DEFAULT_LLAMA_CPP_COMPLETION_BINARY.to_owned()),
            model_path,
        }
    }

    pub fn parse_task(
        &self,
        raw_text: &str,
        project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError> {
        let request_timestamp = format_iso_8601_millis(crate::time_utils::now_utc());
        let prompt = build_llama_cpp_prompt(raw_text, project_catalog);

        // We intentionally use `llama-completion` in single-turn chat mode
        // instead of raw completion mode. The local default model is an
        // instruct-tuned checkpoint, so preserving the system/user separation
        // gives materially better project and priority extraction than stuffing
        // the entire exchange into one flat completion prompt.
        //
        // `-cnv` makes the chat-template choice explicit instead of relying on
        // llama.cpp's auto mode, and `--no-display-prompt` avoids echoing the
        // full prompt back into stdout. We intentionally do not use
        // `--log-disable`: on the local build used for this project it suppresses
        // generated output, not just diagnostics.
        //
        // The output budget is a little larger than before because the model
        // now returns both a concise title and supporting Markdown inside the
        // JSON payload instead of a single summary string.
        //
        // TODO: surface model-specific tuning knobs if we end up supporting
        // multiple local prompt styles. Right now we optimize for one default
        // instruct model path instead of building a full prompt-template system.
        let args = vec![
            "-m".to_owned(),
            path_to_string(&self.model_path),
            "-cnv".to_owned(),
            "-sys".to_owned(),
            prompt.system_prompt,
            "-p".to_owned(),
            prompt.user_prompt,
            "--single-turn".to_owned(),
            "--no-display-prompt".to_owned(),
            "-n".to_owned(),
            "384".to_owned(),
            "--temp".to_owned(),
            "0".to_owned(),
        ];

        let result = Command::new(&self.binary_path)
            .args(&args)
            .output()
            .map_err(|error| {
                log_parse_event(
                    &request_timestamp,
                    &self.binary_path,
                    &self.model_path,
                    false,
                    Some(format!("Could not start llama-completion: {error}")),
                );
                TrackError::new(
                    ErrorCode::AiParseFailed,
                    "AI parse failure. Please try again with a more explicit task description.",
                )
            })?;

        if !result.status.success() {
            let message = format!(
                "AI parse failure. llama-completion exited with code {}.",
                result
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_owned())
            );
            log_parse_event(
                &request_timestamp,
                &self.binary_path,
                &self.model_path,
                false,
                Some(message.clone()),
            );
            return Err(TrackError::new(ErrorCode::AiParseFailed, message));
        }

        let parsed_candidate = parse_candidate_from_command_output(&result.stdout, &result.stderr)
            .map_err(|error| {
                log_parse_event(
                    &request_timestamp,
                    &self.binary_path,
                    &self.model_path,
                    false,
                    Some(error.message().to_owned()),
                );
                error
            })?;

        if env::var("TRACK_DEBUG_AI").ok().as_deref() == Some("1") {
            log_parse_event(
                &request_timestamp,
                &self.binary_path,
                &self.model_path,
                true,
                None,
            );
        }

        Ok(parsed_candidate)
    }
}

fn log_parse_event(
    timestamp: &str,
    binary_path: &str,
    model_path: &std::path::Path,
    ok: bool,
    error: Option<String>,
) {
    let event = json!({
        "timestamp": timestamp,
        "event": "ai_parse",
        "provider": "llama-cpp",
        "binaryPath": binary_path,
        "modelPath": path_to_string(model_path),
        "ok": ok,
        "error": error,
    });

    eprintln!(
        "{}",
        serde_json::to_string(&event).expect("ai parse event serialization should succeed")
    );
}

fn combine_model_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);

    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.into_owned(),
        (true, false) => stderr.into_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn parse_candidate_from_command_output(
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

fn parse_candidate_from_model_output(output: &str) -> Result<ParsedTaskCandidate, TrackError> {
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
    use crate::types::{Confidence, ParsedTaskCandidate, Priority};

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
