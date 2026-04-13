use crate::remote_layout::WorkspaceKey;

define_path_id!(
    ProjectId,
    "Project name",
    "database project names should be valid path components"
);

impl ProjectId {
    pub fn as_workspace_key(&self) -> WorkspaceKey {
        WorkspaceKey::new(self.0.clone()).expect("ProjectID must always be a valid WorkspaceKey")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ProjectId;

    #[test]
    fn trims_and_validates_path_ids() {
        let project_id = ProjectId::new(" project-a ").expect("path ids should validate");

        assert_eq!(project_id.as_str(), "project-a");
    }

    #[test]
    fn serde_rejects_invalid_identifier_values() {
        let error = serde_json::from_value::<ProjectId>(json!("a/b"))
            .expect_err("serde should reject invalid ids");

        assert!(error
            .to_string()
            .contains("must be one non-empty path component"));
    }
}
