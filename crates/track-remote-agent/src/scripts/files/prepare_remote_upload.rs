use crate::scripts::remote_path_helpers_shell;

/// Creates the parent directory needed before uploading a local file to the
/// remote host with `scp`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PrepareRemoteUploadScript;

impl PrepareRemoteUploadScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
REMOTE_PATH="$(expand_remote_path "$1")"
mkdir -p "$(dirname "$REMOTE_PATH")"
"#,
            path_helpers = remote_path_helpers_shell(),
        )
    }

    pub(crate) fn arguments(&self, remote_path: &str) -> Vec<String> {
        vec![remote_path.to_owned()]
    }
}
