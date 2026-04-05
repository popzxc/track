use serde::Serialize;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const PREPARE_REMOTE_UPLOAD_TEMPLATE: &str =
    include_str!("../../../templates/scripts/files/prepare_remote_upload.sh.tera");

/// Creates the parent directory needed before uploading a local file to the
/// remote host with `scp`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PrepareRemoteUploadScript;

impl PrepareRemoteUploadScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            PREPARE_REMOTE_UPLOAD_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(&self, remote_path: &str) -> Vec<String> {
        vec![remote_path.to_owned()]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
