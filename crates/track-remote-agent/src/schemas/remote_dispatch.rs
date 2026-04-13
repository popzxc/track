use serde_json::json;

/// Defines the structured terminal outcome a remote task run must return.
///
/// The remote agent can produce rich free-form reasoning while it works, but
/// the local tracker needs one stable machine-readable summary when the run is
/// done so it can persist the outcome and reconcile follow-up automation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RemoteDispatchSchema;

impl RemoteDispatchSchema {
    pub(crate) fn render(&self) -> String {
        serde_json::to_string_pretty(&json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "status",
                "summary",
                "pullRequestUrl",
                "branchName",
                "worktreePath",
                "notes"
            ],
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["succeeded", "failed", "blocked"]
                },
                "summary": {
                    "type": "string"
                },
                "pullRequestUrl": {
                    "type": ["string", "null"]
                },
                "branchName": {
                    "type": ["string", "null"]
                },
                "worktreePath": {
                    "type": "string"
                },
                "notes": {
                    "type": ["string", "null"]
                }
            }
        }))
        .expect("dispatch schema serialization should succeed")
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteDispatchSchema;

    #[test]
    fn limits_terminal_status_values() {
        let schema = RemoteDispatchSchema.render();

        assert!(schema.contains("\"succeeded\""));
        assert!(schema.contains("\"failed\""));
        assert!(schema.contains("\"blocked\""));
        assert!(schema.contains("\"pullRequestUrl\""));
        assert!(schema.contains("\"branchName\""));
        assert!(schema.contains("\"notes\""));
        assert!(schema.contains("\"required\""));
        assert!(!schema.contains("\"running\""));
    }
}
