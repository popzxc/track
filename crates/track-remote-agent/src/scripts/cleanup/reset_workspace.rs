use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteResetSummary;

use crate::scripts::remote_path_helpers_shell;
use crate::types::RemoteWorkspaceResetReport;

/// Resets the entire remote workspace root and the project registry that maps
/// logical projects to remote checkouts.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ResetWorkspaceScript;

impl ResetWorkspaceScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
WORKSPACE_ROOT="$(expand_remote_path "$1")"
REGISTRY_PATH="$(expand_remote_path "$2")"
WORKSPACE_ENTRIES_REMOVED=0
REGISTRY_REMOVED=false

if [ -z "$WORKSPACE_ROOT" ] || [ "$WORKSPACE_ROOT" = "/" ] || [ "$WORKSPACE_ROOT" = "$HOME" ]; then
  echo "Refusing to reset an unsafe remote workspace root at $WORKSPACE_ROOT." >&2
  exit 1
fi

mkdir -p "$WORKSPACE_ROOT"

for ENTRY in "$WORKSPACE_ROOT"/* "$WORKSPACE_ROOT"/.[!.]* "$WORKSPACE_ROOT"/..?*; do
  [ -e "$ENTRY" ] || continue
  rm -rf "$ENTRY"
  if [ ! -e "$ENTRY" ]; then
    WORKSPACE_ENTRIES_REMOVED=$((WORKSPACE_ENTRIES_REMOVED + 1))
  fi
done

if [ -e "$REGISTRY_PATH" ]; then
  rm -f "$REGISTRY_PATH"
  if [ ! -e "$REGISTRY_PATH" ]; then
    REGISTRY_REMOVED=true
  fi
fi

printf '{{"workspaceEntriesRemoved":%s,"registryRemoved":%s}}\n' \
  "$WORKSPACE_ENTRIES_REMOVED" \
  "$REGISTRY_REMOVED"
"#,
            path_helpers = remote_path_helpers_shell(),
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

        Ok(RemoteResetSummary {
            workspace_entries_removed: parsed_report.workspace_entries_removed,
            registry_removed: parsed_report.registry_removed,
        })
    }
}
