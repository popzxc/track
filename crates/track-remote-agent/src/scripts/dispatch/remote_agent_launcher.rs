use track_types::types::RemoteAgentPreferredTool;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_PROMPT_FILE_NAME, REMOTE_RESULT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME,
    REMOTE_STATUS_FILE_NAME, REMOTE_STDERR_FILE_NAME,
};

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
        let mut launcher = String::from("#!/usr/bin/env bash\n");
        launcher.push_str("set -e\n");
        if !self.shell_prelude.trim().is_empty() {
            launcher.push_str(self.shell_prelude);
            if !self.shell_prelude.ends_with('\n') {
                launcher.push('\n');
            }
        }

        launcher.push_str("set -eu\n");
        launcher.push_str("RUN_DIR=\"$1\"\n");
        launcher.push_str("WORKTREE_PATH=\"$2\"\n");
        launcher.push_str(&format!(
            "printf '%s\\n' \"$$\" > \"$RUN_DIR/{REMOTE_LAUNCHER_PID_FILE_NAME}\"\n"
        ));
        launcher.push_str("cancel_run() {\n");
        launcher.push_str(&format!(
            "  if [ -f \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\" ]; then\n"
        ));
        launcher.push_str(&format!(
            "    CODEX_PID=\"$(tr -d '[:space:]' < \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\")\"\n"
        ));
        launcher
            .push_str("    if [ -n \"$CODEX_PID\" ] && kill -0 \"$CODEX_PID\" 2>/dev/null; then\n");
        launcher.push_str("      kill \"$CODEX_PID\" 2>/dev/null || true\n");
        launcher.push_str("    fi\n");
        launcher.push_str("  fi\n");
        launcher.push_str(&format!(
            "  printf 'canceled\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
        ));
        launcher.push_str(&format!(
            "  date -u +%Y-%m-%dT%H:%M:%SZ > \"$RUN_DIR/{REMOTE_FINISHED_AT_FILE_NAME}\"\n"
        ));
        launcher.push_str("  exit 130\n");
        launcher.push_str("}\n");
        launcher.push_str("trap cancel_run TERM INT\n");
        launcher.push_str(&format!(
            "printf 'running\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
        ));
        launcher.push_str(&build_remote_agent_command(self.preferred_tool));
        launcher.push_str("CODEX_PID=\"$!\"\n");
        launcher.push_str(&format!(
            "printf '%s\\n' \"$CODEX_PID\" > \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\"\n"
        ));
        launcher.push_str("if wait \"$CODEX_PID\"; then\n");
        launcher.push_str(&format!(
            "  printf 'completed\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
        ));
        launcher.push_str("else\n");
        launcher.push_str("  EXIT_CODE=\"$?\"\n");
        launcher.push_str(&format!(
            "  CURRENT_STATUS=\"$(tr -d '[:space:]' < \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\" 2>/dev/null || true)\"\n"
        ));
        launcher.push_str(
            "  if [ \"$CURRENT_STATUS\" != \"canceled\" ] && [ \"$EXIT_CODE\" -ne 130 ]; then\n",
        );
        launcher.push_str(&format!(
            "    printf 'launcher_failed\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
        ));
        launcher.push_str("  fi\n");
        launcher.push_str("fi\n");
        launcher.push_str(&format!(
            "date -u +%Y-%m-%dT%H:%M:%SZ > \"$RUN_DIR/{REMOTE_FINISHED_AT_FILE_NAME}\"\n"
        ));
        launcher
    }
}

fn build_remote_agent_command(preferred_tool: RemoteAgentPreferredTool) -> String {
    match preferred_tool {
        RemoteAgentPreferredTool::Codex => format!(
            "codex --search exec --dangerously-bypass-approvals-and-sandbox -C \"$WORKTREE_PATH\" --json --output-schema \"$RUN_DIR/{REMOTE_SCHEMA_FILE_NAME}\" -o \"$RUN_DIR/{REMOTE_RESULT_FILE_NAME}\" - < \"$RUN_DIR/{REMOTE_PROMPT_FILE_NAME}\" > \"$RUN_DIR/events.jsonl\" 2> \"$RUN_DIR/{REMOTE_STDERR_FILE_NAME}\" &\n"
        ),
        RemoteAgentPreferredTool::Claude => {
            let mut command = String::new();
            command.push_str(&format!(
                "SCHEMA_CONTENT=\"$(tr -d '\\n' < \"$RUN_DIR/{REMOTE_SCHEMA_FILE_NAME}\")\"\n"
            ));
            command.push_str("cd \"$WORKTREE_PATH\"\n");
            command.push_str(&format!(
                "claude -p --dangerously-skip-permissions --add-dir \"$WORKTREE_PATH\" --output-format json --json-schema \"$SCHEMA_CONTENT\" < \"$RUN_DIR/{REMOTE_PROMPT_FILE_NAME}\" > \"$RUN_DIR/{REMOTE_RESULT_FILE_NAME}\" 2> \"$RUN_DIR/{REMOTE_STDERR_FILE_NAME}\" &\n"
            ));
            command
        }
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

        assert!(launcher.starts_with("#!/usr/bin/env bash"));
        assert!(launcher.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(launcher.contains("codex --search exec"));
        assert!(launcher.contains("RUN_DIR=\"$1\""));
        assert!(launcher.contains("WORKTREE_PATH=\"$2\""));
        assert!(launcher.contains("launcher.pid"));
        assert!(launcher.contains("codex.pid"));
        assert!(launcher.contains("trap cancel_run TERM INT"));
        assert!(launcher.contains("canceled"));
    }

    #[test]
    fn builds_claude_launcher_with_schema_validation_and_yolo_mode() {
        let launcher = RemoteAgentLauncherScript::new(
            RemoteAgentPreferredTool::Claude,
            "export PATH=\"$HOME/.local/bin:$PATH\"\n",
        )
        .render();

        assert!(launcher.starts_with("#!/usr/bin/env bash"));
        assert!(launcher.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
        assert!(launcher.contains("SCHEMA_CONTENT=\"$(tr -d '\\n'"));
        assert!(launcher.contains("cd \"$WORKTREE_PATH\""));
        assert!(launcher.contains("claude -p --dangerously-skip-permissions"));
        assert!(launcher.contains("--output-format json"));
        assert!(launcher.contains("--json-schema \"$SCHEMA_CONTENT\""));
        assert!(launcher.contains("codex.pid"));
    }
}
