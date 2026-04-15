//! Types that represent remote dispatch state and remote workspace bookkeeping.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{RemoteAgentPreferredTool, RemoteResetSummary};

/// Normalized summary of what a remote cleanup operation actually removed.
///
/// Higher layers use these counts to report cleanup outcomes without leaking the
/// exact JSON shape emitted by the remote helper scripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteArtifactCleanupCounts {
    pub(crate) worktrees_removed: usize,
    pub(crate) run_directories_removed: usize,
}

impl From<RemoteArtifactCleanupReport> for RemoteArtifactCleanupCounts {
    fn from(report: RemoteArtifactCleanupReport) -> Self {
        Self {
            worktrees_removed: report.worktrees_removed,
            run_directories_removed: report.run_directories_removed,
        }
    }
}

/// Raw cleanup report returned by the remote helper script.
///
/// This exists so the script and the Rust code can evolve independently while
/// still sharing a clear, typed contract at the process boundary.
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteArtifactCleanupReport {
    #[serde(rename = "worktreesRemoved")]
    pub(crate) worktrees_removed: usize,
    #[serde(rename = "runDirectoriesRemoved")]
    pub(crate) run_directories_removed: usize,
}

/// Raw report returned by the remote workspace reset helper script.
///
/// The report captures how much persisted remote workspace state was removed so
/// callers can explain the reset outcome without parsing ad hoc shell output.
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteWorkspaceResetReport {
    #[serde(rename = "workspaceEntriesRemoved")]
    pub(crate) workspace_entries_removed: usize,
    #[serde(rename = "registryRemoved")]
    pub(crate) registry_removed: bool,
}

impl RemoteWorkspaceResetReport {
    pub(crate) fn into_summary(self) -> RemoteResetSummary {
        RemoteResetSummary {
            workspace_entries_removed: self.workspace_entries_removed,
            registry_removed: self.registry_removed,
        }
    }
}

/// Describes how aggressively remote artifacts should be removed when a task
/// leaves the active workflow.
///
/// Closing a task and deleting a task are different user intents, so the remote
/// cleanup layer needs an explicit mode instead of inferring semantics from
/// callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteTaskCleanupMode {
    CloseTask,
    DeleteTask,
}

/// Wrapper used when a Claude run returns typed data under a
/// `structured_output` envelope.
///
/// This lets the rest of the crate deserialize the meaningful payload type
/// directly while keeping the provider-specific outer shape at the boundary.
#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeStructuredOutputEnvelope<T> {
    #[serde(rename = "structured_output")]
    pub(crate) structured_output: T,
}

impl<T> ClaudeStructuredOutputEnvelope<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn parse_result(
        raw_result: &str,
        preferred_tool: RemoteAgentPreferredTool,
        result_label: &str,
    ) -> Result<T, TrackError> {
        match serde_json::from_str::<T>(raw_result) {
            Ok(outcome) => Ok(outcome),
            Err(direct_error) if preferred_tool == RemoteAgentPreferredTool::Claude => {
                serde_json::from_str::<ClaudeStructuredOutputEnvelope<T>>(raw_result)
                    .map(|envelope| envelope.structured_output)
                    .map_err(|envelope_error| {
                        TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            format!(
                                "{result_label} did not match the expected direct or Claude structured-output format: direct parse failed with {direct_error}; envelope parse failed with {envelope_error}",
                            ),
                        )
                    })
            }
            Err(error) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("{result_label} is not valid JSON: {error}"),
            )),
        }
    }
}

/// Parser for opencode's JSON event stream output.
///
/// Opencode outputs a stream of JSON events (one per line):
/// - step_start: Beginning of a reasoning step
/// - tool_use: Tool execution details
/// - text: Final text response (what we need)
/// - step_finish: End of a reasoning step
///
/// We extract the last `text` event and parse it as the structured outcome.
///
/// TODO: Consider capturing intermediate events for debugging/auditing.
#[derive(Debug, Deserialize)]
struct OpencodeTextEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    part: Option<OpencodeEventPart>,
}

#[derive(Debug, Deserialize)]
struct OpencodeEventPart {
    #[serde(default)]
    text: Option<String>,
}

pub(crate) struct OpencodeStructuredOutput<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> OpencodeStructuredOutput<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn parse_result(raw_result: &str, result_label: &str) -> Result<T, TrackError> {
        let mut final_text: Option<String> = None;

        for (line_num, line) in raw_result.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<OpencodeTextEvent>(line) {
                Ok(event) if event.event_type == "text" => {
                    if let Some(part) = event.part {
                        final_text = part.text;
                    }
                }
                Ok(_) => {
                    // Other event types (step_start, tool_use, etc.) - ignore for now
                    // TODO: Consider logging these for debugging
                }
                Err(_) => {
                    // Malformed JSON line - log and continue
                    tracing::debug!(
                        line_num = line_num,
                        line = %line,
                        "Opencode event stream contained malformed JSON"
                    );
                }
            }
        }

        let text = final_text.ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "{result_label} did not contain any text events in the opencode event stream. \
                     Raw output:\n{}",
                    truncate_output(raw_result, 2000)
                ),
            )
        })?;

        // Parse the extracted text as JSON
        serde_json::from_str::<T>(&text).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "{result_label} text was not valid JSON: {error}. \
                     Extracted text:\n{}\n\n\
                     Full raw output:\n{}",
                    truncate_output(&text, 1000),
                    truncate_output(raw_result, 2000)
                ),
            )
        })
    }
}

fn truncate_output(output: &str, max_len: usize) -> String {
    // TODO: make it a single pass
    let output_char_count = output.chars().count();
    if output_char_count <= max_len {
        output.to_owned()
    } else {
        let truncated = output.chars().take(max_len).collect::<String>();
        // TODO: Consider providing a way to view the full raw output in the UI for debugging.
        format!("{truncated}... ({} bytes total)", output.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use track_types::types::{DispatchStatus, RemoteAgentDispatchOutcome};

    #[test]
    fn parses_opencode_event_stream_with_single_text_event() {
        let event_stream = r#"{"type":"step_start","timestamp":123,"sessionID":"ses_1","part":{"type":"step-start"}}
{"type":"step_finish","timestamp":124,"sessionID":"ses_1","part":{"type":"step-finish"}}
{"type":"step_start","timestamp":125,"sessionID":"ses_1","part":{"type":"step-start"}}
{"type":"text","timestamp":126,"sessionID":"ses_1","part":{"type":"text","text":"{\"status\":\"succeeded\",\"summary\":\"Test summary\",\"worktreePath\":\"/home/track/workspace/project-a/worktrees/dispatch-abc123\"}"}}
{"type":"step_finish","timestamp":127,"sessionID":"ses_1","part":{"type":"step-finish"}}"#;

        let result = OpencodeStructuredOutput::<RemoteAgentDispatchOutcome>::parse_result(
            event_stream,
            "Test result",
        )
        .expect("should parse valid event stream");

        assert_eq!(result.status, DispatchStatus::Succeeded);
        assert_eq!(result.summary, "Test summary");
    }

    #[test]
    fn parses_opencode_event_stream_with_multiple_text_events_uses_last() {
        let event_stream = r#"{"type":"text","part":{"text":"{\"status\":\"failed\",\"summary\":\"First\",\"worktreePath\":\"/home/track/workspace/project-a/worktrees/dispatch-abc123\"}"}}
{"type":"text","part":{"text":"{\"status\":\"succeeded\",\"summary\":\"Second\",\"worktreePath\":\"/home/track/workspace/project-a/worktrees/dispatch-abc123\"}"}}"#;

        let result = OpencodeStructuredOutput::<RemoteAgentDispatchOutcome>::parse_result(
            event_stream,
            "Test result",
        )
        .expect("should use last text event");

        assert_eq!(result.status, DispatchStatus::Succeeded);
        assert_eq!(result.summary, "Second");
    }

    #[test]
    fn fails_when_no_text_event_present() {
        let event_stream = r#"{"type":"step_start","part":{"type":"step-start"}}
{"type":"step_finish","part":{"type":"step-finish"}}"#;

        let result = OpencodeStructuredOutput::<RemoteAgentDispatchOutcome>::parse_result(
            event_stream,
            "Test result",
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message().contains("did not contain any text events"));
    }

    #[test]
    fn fails_when_text_is_not_valid_json() {
        let event_stream = r#"{"type":"text","part":{"text":"not valid json"}}"#;

        let result = OpencodeStructuredOutput::<RemoteAgentDispatchOutcome>::parse_result(
            event_stream,
            "Test result",
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message().contains("not valid JSON"));
    }

    #[test]
    fn truncates_long_raw_output_in_error() {
        let long_output = "x".repeat(3000);
        let event_stream = format!(r#"{{"type":"text","part":{{"text":"not json"}}}}"#);
        let full_stream = format!("{}\n{}", event_stream, long_output);

        let result = OpencodeStructuredOutput::<RemoteAgentDispatchOutcome>::parse_result(
            &full_stream,
            "Test result",
        );

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.message().contains("... ("));
        assert!(error.message().contains("bytes total"));
    }

    #[test]
    fn truncates_unicode_output_without_panicking() {
        let long_unicode = "🙂".repeat(3000);
        let truncated = truncate_output(&long_unicode, 2000);

        assert!(truncated.starts_with(&"🙂".repeat(2000)));
        assert!(truncated.contains("bytes total"));
    }
}
