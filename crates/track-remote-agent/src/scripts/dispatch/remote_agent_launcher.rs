use serde::Serialize;
use track_types::types::RemoteAgentPreferredTool;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_PROMPT_FILE_NAME, REMOTE_RESULT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME,
    REMOTE_STATUS_FILE_NAME, REMOTE_STDERR_FILE_NAME,
};
use crate::template_renderer::render_template;

const REMOTE_AGENT_LAUNCHER_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/remote_agent_launcher.sh.tera");
const CODEX_LAUNCH_COMMAND_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/remote_agent_launcher_codex_command.sh.tera");
const CLAUDE_LAUNCH_COMMAND_TEMPLATE: &str = include_str!(
    "../../../templates/scripts/dispatch/remote_agent_launcher_claude_command.sh.tera"
);

/// Renders the launcher script that actually runs Codex or Claude in the remote
/// worktree.
///
/// The launcher is the durable contract between queueing a run and the remote
/// process that writes status, result, and stderr artifacts as the run
/// progresses.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RemoteAgentLauncherScript<'a> {
    preferred_tool: RemoteAgentPreferredTool,
    shell_prelude: &'a str,
}

impl<'a> RemoteAgentLauncherScript<'a> {
    pub(crate) fn new(preferred_tool: RemoteAgentPreferredTool, shell_prelude: &'a str) -> Self {
        Self {
            preferred_tool,
            shell_prelude,
        }
    }

    pub(crate) fn render(&self) -> String {
        let template_context = RemoteAgentLauncherTemplate {
            shell_prelude: normalize_shell_prelude(self.shell_prelude).into_owned(),
            launcher_pid_file: REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file: REMOTE_CODEX_PID_FILE_NAME,
            status_file: REMOTE_STATUS_FILE_NAME,
            finished_at_file: REMOTE_FINISHED_AT_FILE_NAME,
            agent_command: build_remote_agent_command(self.preferred_tool),
        };

        render_template(REMOTE_AGENT_LAUNCHER_TEMPLATE, &template_context)
    }
}

fn build_remote_agent_command(preferred_tool: RemoteAgentPreferredTool) -> String {
    let template_context = RemoteAgentCommandTemplate {
        schema_file: REMOTE_SCHEMA_FILE_NAME,
        result_file: REMOTE_RESULT_FILE_NAME,
        prompt_file: REMOTE_PROMPT_FILE_NAME,
        stderr_file: REMOTE_STDERR_FILE_NAME,
    };

    match preferred_tool {
        RemoteAgentPreferredTool::Codex => {
            render_template(CODEX_LAUNCH_COMMAND_TEMPLATE, &template_context)
        }
        RemoteAgentPreferredTool::Claude => {
            render_template(CLAUDE_LAUNCH_COMMAND_TEMPLATE, &template_context)
        }
    }
}

#[derive(Serialize)]
struct RemoteAgentLauncherTemplate<'a> {
    shell_prelude: String,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
    status_file: &'a str,
    finished_at_file: &'a str,
    agent_command: String,
}

#[derive(Serialize)]
struct RemoteAgentCommandTemplate<'a> {
    schema_file: &'a str,
    result_file: &'a str,
    prompt_file: &'a str,
    stderr_file: &'a str,
}

fn normalize_shell_prelude(shell_prelude: &str) -> std::borrow::Cow<'_, str> {
    if shell_prelude.trim().is_empty() {
        std::borrow::Cow::Borrowed("")
    } else if shell_prelude.ends_with('\n') {
        std::borrow::Cow::Borrowed(shell_prelude)
    } else {
        std::borrow::Cow::Owned(format!("{shell_prelude}\n"))
    }
}

#[cfg(test)]
mod tests {
    use track_types::types::RemoteAgentPreferredTool;

    use super::RemoteAgentLauncherScript;

    #[test]
    fn builds_codex_launcher_with_runner_shell_prelude() {
        let launcher = RemoteAgentLauncherScript::new(
            RemoteAgentPreferredTool::Codex,
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"\n",
        )
        .render();

        insta::assert_snapshot!("remote_agent_launcher_codex", launcher);
    }

    #[test]
    fn builds_claude_launcher_with_schema_validation_and_yolo_mode() {
        let launcher = RemoteAgentLauncherScript::new(
            RemoteAgentPreferredTool::Claude,
            "export PATH=\"$HOME/.local/bin:$PATH\"\n",
        )
        .render();

        insta::assert_snapshot!("remote_agent_launcher_claude", launcher);
    }
}
