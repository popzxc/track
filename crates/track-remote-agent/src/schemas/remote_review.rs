use serde_json::json;

/// Defines the structured terminal outcome a remote review run must return.
///
/// Review runs are expected to leave behind durable GitHub review metadata, so
/// this schema makes the submission result explicit instead of forcing the
/// caller to infer it from free-form output text.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RemoteReviewSchema;

impl RemoteReviewSchema {
    pub(crate) fn render(&self) -> String {
        serde_json::to_string_pretty(&json!({
            "type": "object",
            "additionalProperties": false,
            "required": [
                "status",
                "summary",
                "reviewSubmitted",
                "githubReviewId",
                "githubReviewUrl",
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
                "reviewSubmitted": {
                    "type": "boolean"
                },
                "githubReviewId": {
                    "type": ["string", "null"]
                },
                "githubReviewUrl": {
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
        .expect("review schema serialization should succeed")
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteReviewSchema;

    #[test]
    fn requires_review_submission_metadata_and_terminal_status_values() {
        let schema = RemoteReviewSchema.render();

        assert!(schema.contains("\"reviewSubmitted\""));
        assert!(schema.contains("\"githubReviewId\""));
        assert!(schema.contains("\"githubReviewUrl\""));
        assert!(schema.contains("\"succeeded\""));
        assert!(schema.contains("\"failed\""));
        assert!(schema.contains("\"blocked\""));
        assert!(!schema.contains("\"running\""));
    }
}
