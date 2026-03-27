use std::collections::HashSet;
use std::env;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{
    AddBos, GrammarTriggerType, LlamaChatMessage, LlamaChatTemplate, LlamaModel,
};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::TokenToStringError;
use llama_cpp_2::{send_logs_to_tracing, LogOptions};
use serde_json::json;
use track_core::errors::{ErrorCode, TrackError};
use track_core::paths::path_to_string;
use track_core::project_catalog::ProjectCatalog;
use track_core::time_utils::format_iso_8601_millis;
use track_core::types::ParsedTaskCandidate;

use crate::prompt::{build_llama_cpp_prompt, build_task_parser_json_schema};
use crate::task_parser::TaskParser;

const MAX_GENERATED_TOKENS: u32 = 384;

pub struct LlamaCpp2TaskParser {
    model_path: PathBuf,
}

impl LlamaCpp2TaskParser {
    pub fn new(model_path: PathBuf) -> Self {
        Self { model_path }
    }
}

impl TaskParser for LlamaCpp2TaskParser {
    fn parse_task(
        &self,
        raw_text: &str,
        project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError> {
        let request_timestamp = format_iso_8601_millis(track_core::time_utils::now_utc());
        let prompt = build_llama_cpp_prompt(raw_text, project_catalog);
        let schema = serde_json::to_string(&build_task_parser_json_schema(project_catalog))
            .expect("task parser schema serialization should succeed");

        // The bindings backend keeps inference in-process, but we intentionally
        // preserve the same system/user prompt contract as the subprocess path.
        // That lets us evaluate the backend swap on generation quality and
        // structured-output reliability without also changing prompt wording.
        let messages = [
            LlamaChatMessage::new("system".to_owned(), prompt.system_prompt).map_err(|error| {
                TrackError::new(
                    ErrorCode::AiParseFailed,
                    format!("AI parse failure. Could not build the system prompt message: {error}"),
                )
            })?,
            LlamaChatMessage::new("user".to_owned(), prompt.user_prompt).map_err(|error| {
                TrackError::new(
                    ErrorCode::AiParseFailed,
                    format!("AI parse failure. Could not build the user prompt message: {error}"),
                )
            })?,
        ];

        let debug_ai = env::var("TRACK_DEBUG_AI").ok().as_deref() == Some("1");
        let parse_result =
            run_llama_cpp_2_inference(&self.model_path, &messages, &schema, debug_ai);

        match parse_result {
            Ok(candidate) => {
                if debug_ai {
                    log_parse_event(&request_timestamp, &self.model_path, true, None);
                }
                Ok(candidate)
            }
            Err(error) => {
                log_parse_event(
                    &request_timestamp,
                    &self.model_path,
                    false,
                    Some(error.message().to_owned()),
                );
                Err(error)
            }
        }
    }
}

fn run_llama_cpp_2_inference(
    model_path: &Path,
    messages: &[LlamaChatMessage],
    json_schema: &str,
    debug_ai: bool,
) -> Result<ParsedTaskCandidate, TrackError> {
    // CUDA device discovery logs are emitted during backend initialization, so
    // suppressing logs after `LlamaBackend::init()` is too late for the normal
    // CLI path. We install the muted callback first and keep the debug path
    // untouched so `TRACK_DEBUG_AI=1` still surfaces upstream details when
    // someone is actively diagnosing local-model behavior.
    //
    // TODO: If `track` ever reuses the parser across multiple captures in one
    // process with different debug settings, replace this crate-level helper
    // with a direct per-call logger hook.
    if !debug_ai {
        send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
    }

    let backend = LlamaBackend::init().map_err(|error| {
        TrackError::new(
            ErrorCode::AiParseFailed,
            format!("AI parse failure. Could not initialize llama.cpp: {error}"),
        )
    })?;

    // A CUDA-enabled build is an explicit performance choice. If that build
    // reaches a machine where libllama cannot offload to a GPU, we fail fast
    // instead of silently running the much slower CPU path.
    let gpu_offload_available = backend.supports_gpu_offload();
    if debug_ai {
        eprintln!(
            "track AI debug: llama.cpp build flavor = {}, gpu offload available = {}",
            llama_cpp_build_flavor(),
            gpu_offload_available,
        );
    }
    #[cfg(feature = "cuda")]
    if !gpu_offload_available {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. This `track` binary was built with CUDA support, but llama.cpp could not enable GPU offload on this machine. Rebuild without `--features cuda` or make sure the NVIDIA driver and CUDA toolkit are installed and the GPU is visible.",
        ));
    }

    let model = LlamaModel::load_from_file(&backend, model_path, &LlamaModelParams::default())
        .map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!(
                    "AI parse failure. Could not load the local model from {}: {error}",
                    path_to_string(model_path)
                ),
            )
        })?;

    let template = match model.chat_template(None) {
        Ok(template) => template,
        Err(_) => LlamaChatTemplate::new("chatml").map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!(
                    "AI parse failure. Could not resolve a chat template for the local model: {error}"
                ),
            )
        })?,
    };

    let result = model
        .apply_chat_template_with_tools_oaicompat(
            &template,
            messages,
            None,
            Some(json_schema),
            true,
        )
        .map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. Could not apply the local model chat template: {error}"),
            )
        })?;

    let tokens = model
        .str_to_token(&result.prompt, AddBos::Always)
        .map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. Could not tokenize the local model prompt: {error}"),
            )
        })?;
    if tokens.is_empty() {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. The local model prompt produced no tokens.",
        ));
    }

    let n_ctx = model
        .n_ctx_train()
        .max(tokens.len() as u32 + MAX_GENERATED_TOKENS);
    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(n_ctx);
    let mut context = model
        .new_context(&backend, context_params)
        .map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. Could not initialize the local model context: {error}"),
            )
        })?;

    let mut batch = LlamaBatch::new(n_ctx as usize, 1);
    let last_prompt_index = tokens.len().saturating_sub(1) as i32;
    for (index, token) in (0_i32..).zip(tokens.iter().copied()) {
        let is_last = index == last_prompt_index;
        batch.add(token, index, &[0], is_last).map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!(
                    "AI parse failure. Could not prepare the local model prompt batch: {error}"
                ),
            )
        })?;
    }

    context.decode(&mut batch).map_err(|error| {
        TrackError::new(
            ErrorCode::AiParseFailed,
            format!("AI parse failure. The local model could not decode the prompt: {error}"),
        )
    })?;

    let (mut sampler, preserved_tokens) = build_sampler(&model, &result)?;
    let mut generated_bytes = Vec::new();
    let mut n_cur = batch.n_tokens();
    let max_tokens_total = n_cur + MAX_GENERATED_TOKENS as i32;

    while n_cur < max_tokens_total {
        let token = sampler.sample(&context, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }

        let output_bytes = decode_token_bytes(&model, token, preserved_tokens.contains(&token))?;
        generated_bytes.extend_from_slice(&output_bytes);
        sampler.accept(token);

        if output_has_stop_sequence(&generated_bytes, &result.additional_stops) {
            break;
        }

        batch.clear();
        batch.add(token, n_cur, &[0], true).map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. Could not extend the local model batch: {error}"),
            )
        })?;
        n_cur += 1;
        context.decode(&mut batch).map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. The local model could not continue decoding: {error}"),
            )
        })?;
    }

    let mut generated_text = String::from_utf8(generated_bytes).map_err(|error| {
        TrackError::new(
            ErrorCode::AiParseFailed,
            format!("AI parse failure. The local model returned invalid UTF-8 output: {error}"),
        )
    })?;
    truncate_additional_stop_sequences(&mut generated_text, &result.additional_stops);

    parse_candidate_from_generated_json(&generated_text)
}

fn llama_cpp_build_flavor() -> &'static str {
    #[cfg(feature = "cuda")]
    {
        "cuda"
    }

    #[cfg(not(feature = "cuda"))]
    {
        "default"
    }
}

fn parse_candidate_from_generated_json(output: &str) -> Result<ParsedTaskCandidate, TrackError> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. The local model returned an empty response.",
        ));
    }

    // The in-process path only sees generated tokens, not the mixed
    // stdout/stderr transcript that the old subprocess flow produced. In local
    // probing with the current cached model we also observed clean top-level
    // JSON objects, so direct deserialization is the simplest contract here.
    serde_json::from_str::<ParsedTaskCandidate>(trimmed).map_err(|error| {
        TrackError::new(
            ErrorCode::AiParseFailed,
            format!("AI parse failure. The local model did not return valid task JSON: {error}"),
        )
    })
}

fn build_sampler(
    model: &LlamaModel,
    result: &llama_cpp_2::model::ChatTemplateResult,
) -> Result<(LlamaSampler, HashSet<LlamaToken>), TrackError> {
    let mut preserved_tokens = HashSet::new();
    for token_str in &result.preserved_tokens {
        let tokens =
            model
                .str_to_token(token_str, AddBos::Never)
                .map_err(|error| {
                    TrackError::new(
                ErrorCode::AiParseFailed,
                format!("AI parse failure. Could not tokenize a preserved grammar token: {error}"),
            )
                })?;
        if tokens.len() == 1 {
            preserved_tokens.insert(tokens[0]);
        }
    }

    let grammar_sampler = if let Some(grammar) = result.grammar.as_deref() {
        if result.grammar_lazy {
            if result.grammar_triggers.is_empty() {
                return Err(TrackError::new(
                    ErrorCode::AiParseFailed,
                    "AI parse failure. The local model returned a lazy grammar without triggers.",
                ));
            }

            let mut trigger_patterns = Vec::new();
            let mut trigger_tokens = Vec::new();
            for trigger in &result.grammar_triggers {
                match trigger.trigger_type {
                    GrammarTriggerType::Token => {
                        if let Some(token) = trigger.token {
                            trigger_tokens.push(token);
                        }
                    }
                    GrammarTriggerType::Word => {
                        let tokens =
                            model.str_to_token(&trigger.value, AddBos::Never).map_err(|error| {
                                TrackError::new(
                                    ErrorCode::AiParseFailed,
                                    format!("AI parse failure. Could not tokenize a lazy grammar trigger: {error}"),
                                )
                            })?;
                        if tokens.len() == 1 {
                            if !preserved_tokens.contains(&tokens[0]) {
                                return Err(TrackError::new(
                                    ErrorCode::AiParseFailed,
                                    format!(
                                        "AI parse failure. The local model returned an inconsistent lazy grammar trigger `{}`.",
                                        trigger.value
                                    ),
                                ));
                            }
                            trigger_tokens.push(tokens[0]);
                        } else {
                            trigger_patterns.push(regex_escape(&trigger.value));
                        }
                    }
                    GrammarTriggerType::Pattern => {
                        trigger_patterns.push(trigger.value.clone());
                    }
                    GrammarTriggerType::PatternFull => {
                        trigger_patterns.push(anchor_pattern(&trigger.value));
                    }
                }
            }

            Some(
                LlamaSampler::grammar_lazy_patterns(
                    model,
                    grammar,
                    "root",
                    &trigger_patterns,
                    &trigger_tokens,
                )
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::AiParseFailed,
                        format!("AI parse failure. Could not initialize the local model grammar sampler: {error}"),
                    )
                })?,
            )
        } else {
            Some(LlamaSampler::grammar(model, grammar, "root").map_err(|error| {
                TrackError::new(
                    ErrorCode::AiParseFailed,
                    format!("AI parse failure. Could not initialize the local model grammar sampler: {error}"),
                )
            })?)
        }
    } else {
        None
    };

    let sampler = match grammar_sampler {
        Some(grammar_sampler) => {
            LlamaSampler::chain_simple([grammar_sampler, LlamaSampler::greedy()])
        }
        None => LlamaSampler::greedy(),
    };

    Ok((sampler, preserved_tokens))
}

fn decode_token_bytes(
    model: &LlamaModel,
    token: LlamaToken,
    preserve_special_token: bool,
) -> Result<Vec<u8>, TrackError> {
    let special = preserve_special_token;
    match model.token_to_piece_bytes(token, 8, special, None) {
        Ok(bytes) => Ok(bytes),
        Err(TokenToStringError::InsufficientBufferSpace(required_size)) => model
            .token_to_piece_bytes(
                token,
                (-required_size)
                    .try_into()
                    .expect("llama.cpp should return a positive buffer size"),
                special,
                None,
            )
            .map_err(token_decode_error),
        Err(error) => Err(token_decode_error(error)),
    }
}

fn token_decode_error(error: TokenToStringError) -> TrackError {
    TrackError::new(
        ErrorCode::AiParseFailed,
        format!("AI parse failure. Could not decode a generated local-model token: {error}"),
    )
}

fn output_has_stop_sequence(output_bytes: &[u8], stops: &[String]) -> bool {
    let Ok(output_text) = std::str::from_utf8(output_bytes) else {
        return false;
    };

    stops
        .iter()
        .any(|stop| !stop.is_empty() && output_text.ends_with(stop))
}

fn truncate_additional_stop_sequences(output: &mut String, stops: &[String]) {
    for stop in stops {
        if !stop.is_empty() && output.ends_with(stop) {
            let new_len = output.len().saturating_sub(stop.len());
            output.truncate(new_len);
            break;
        }
    }
}

fn regex_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '.' | '^' | '$' | '|' | '(' | ')' | '*' | '+' | '?' | '[' | ']' | '{' | '}' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

fn anchor_pattern(pattern: &str) -> String {
    if pattern.is_empty() {
        return "^$".to_owned();
    }

    let mut anchored = String::new();
    if !pattern.starts_with('^') {
        anchored.push('^');
    }
    anchored.push_str(pattern);
    if !pattern.ends_with('$') {
        anchored.push('$');
    }
    anchored
}

fn log_parse_event(timestamp: &str, model_path: &Path, ok: bool, error: Option<String>) {
    let event = json!({
        "timestamp": timestamp,
        "event": "ai_parse",
        "provider": "llama-cpp-2",
        "buildFlavor": llama_cpp_build_flavor(),
        "modelPath": path_to_string(model_path),
        "ok": ok,
        "error": error,
    });

    eprintln!(
        "{}",
        serde_json::to_string(&event).expect("ai parse event serialization should succeed")
    );
}

#[cfg(test)]
mod tests {
    use track_core::types::{Confidence, Priority};

    use super::parse_candidate_from_generated_json;

    #[test]
    fn parses_clean_generated_json_directly() {
        let candidate = parse_candidate_from_generated_json(
            r#"{
  "project": "project-x",
  "priority": "high",
  "title": "Fix a bug",
  "bodyMarkdown": "",
  "confidence": "high",
  "reason": null
}"#,
        )
        .expect("clean generated json should parse");

        assert_eq!(candidate.project.as_deref(), Some("project-x"));
        assert_eq!(candidate.priority, Priority::High);
        assert_eq!(candidate.title, "Fix a bug");
        assert_eq!(candidate.body_markdown.as_deref(), Some(""));
        assert_eq!(candidate.confidence, Confidence::High);
        assert_eq!(candidate.reason, None);
    }

    #[test]
    fn rejects_empty_generated_output() {
        let error =
            parse_candidate_from_generated_json(" \n\t ").expect_err("empty output should fail");

        assert!(error.message().contains("empty response"));
    }
}
