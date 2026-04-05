use serde::Serialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteResetSummary;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;
use crate::types::RemoteWorkspaceResetReport;

const RESET_WORKSPACE_TEMPLATE: &str =
    include_str!("../../../templates/scripts/cleanup/reset_workspace.sh.tera");

/// Resets the entire remote workspace root and the project registry that maps
/// logical projects to remote checkouts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ResetWorkspaceScript;

impl ResetWorkspaceScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            RESET_WORKSPACE_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        workspace_root: &str,
        projects_registry_path: &str,
    ) -> Vec<String> {
        vec![workspace_root.to_owned(), projects_registry_path.to_owned()]
    }

    pub(crate) fn parse_report(&self, report: &str) -> Result<RemoteResetSummary, TrackError> {
        let parsed_report = serde_json::from_str::<RemoteWorkspaceResetReport>(report.trim())
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Could not parse the remote reset report: {error}"),
                )
            })?;

        Ok(parsed_report.into_summary())
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
