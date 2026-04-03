use crate::scripts::remote_path_helpers_shell;

/// Reads a single remote file if it exists and signals a distinct exit code
/// when it does not.
///
/// The missing-file exit code lets the caller distinguish "not there yet" from
/// genuine remote execution failures.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReadRemoteFileScript;

impl ReadRemoteFileScript {
    pub(crate) const MISSING_FILE_EXIT_CODE: i32 = 3;

    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
REMOTE_PATH="$(expand_remote_path "$1")"
if [ -f "$REMOTE_PATH" ]; then
  cat "$REMOTE_PATH"
else
  exit {missing_exit_code}
fi
"#,
            path_helpers = remote_path_helpers_shell(),
            missing_exit_code = Self::MISSING_FILE_EXIT_CODE,
        )
    }

    pub(crate) fn arguments(&self, remote_path: &str) -> Vec<String> {
        vec![remote_path.to_owned()]
    }
}
