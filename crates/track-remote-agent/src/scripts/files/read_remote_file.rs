use serde::Serialize;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const READ_REMOTE_FILE_TEMPLATE: &str =
    include_str!("../../../templates/scripts/files/read_remote_file.sh.tera");

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
        render_template(
            READ_REMOTE_FILE_TEMPLATE,
            &ReadRemoteFileTemplate {
                path_helpers: remote_path_helpers_shell(),
                missing_exit_code: Self::MISSING_FILE_EXIT_CODE,
            },
        )
    }

    pub(crate) fn arguments(&self, remote_path: &str) -> Vec<String> {
        vec![remote_path.to_owned()]
    }
}

#[derive(Serialize)]
struct ReadRemoteFileTemplate<'a> {
    path_helpers: &'a str,
    missing_exit_code: i32,
}
