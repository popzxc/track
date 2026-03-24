use std::env;
use std::process::Command;

use serde_json::json;
use track_core::errors::{ErrorCode, TrackError};
use track_core::paths::path_to_string;
use track_core::project_catalog::ProjectCatalog;
use track_core::time_utils::format_iso_8601_millis;
use track_core::types::ParsedTaskCandidate;

use crate::prompt::{build_llama_cpp_prompt, DEFAULT_LLAMA_CPP_COMPLETION_BINARY};
use crate::task_parser::TaskParser;
use crate::task_parser_output::parse_candidate_from_command_output;

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
}

impl TaskParser for LlamaCppTaskParser {
    fn parse_task(
        &self,
        raw_text: &str,
        project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError> {
        let request_timestamp = format_iso_8601_millis(track_core::time_utils::now_utc());
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
