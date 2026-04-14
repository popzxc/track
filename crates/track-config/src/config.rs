use serde::{Deserialize, Serialize};
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteAgentPreferredTool;

pub const DEFAULT_API_PORT: u16 = 3210;
pub const DEFAULT_REMOTE_AGENT_PORT: u16 = 22;
pub const DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT: &str = "~/workspace";
pub const DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH: &str = "~/track-projects.json";
pub const DEFAULT_LLAMACPP_MODEL_HF_REPO: &str = "bartowski/Meta-Llama-3-8B-Instruct-GGUF";
pub const DEFAULT_LLAMACPP_MODEL_HF_FILE: &str = "Meta-Llama-3-8B-Instruct-Q4_K_M.gguf";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaCppConfigFile {
    #[serde(rename = "modelPath", default, skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
    #[serde(
        rename = "modelHfRepo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub model_hf_repo: Option<String>,
    #[serde(
        rename = "modelHfFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub model_hf_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentConfigFile {
    pub host: String,
    pub user: String,
    #[serde(default = "default_remote_agent_port")]
    pub port: u16,
    #[serde(
        rename = "workspaceRoot",
        default = "default_remote_agent_workspace_root"
    )]
    pub workspace_root: String,
    #[serde(
        rename = "projectsRegistryPath",
        default = "default_remote_projects_registry_path"
    )]
    pub projects_registry_path: String,
    #[serde(
        rename = "preferredTool",
        default,
        skip_serializing_if = "RemoteAgentPreferredTool::is_codex"
    )]
    pub preferred_tool: RemoteAgentPreferredTool,
    #[serde(
        rename = "shellPrelude",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shell_prelude: Option<String>,
    #[serde(
        rename = "reviewFollowUp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentReviewFollowUpConfigFile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "mainUser", default, skip_serializing_if = "Option::is_none")]
    pub main_user: Option<String>,
    #[serde(
        rename = "defaultReviewPrompt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_review_prompt: Option<String>,
}

fn default_remote_agent_port() -> u16 {
    DEFAULT_REMOTE_AGENT_PORT
}

fn default_remote_agent_workspace_root() -> String {
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned()
}

fn default_remote_projects_registry_path() -> String {
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned()
}

pub fn canonicalize_optional_multiline_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.replace("\r\n", "\n").trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn canonicalize_remote_agent_config(
    remote_agent: RemoteAgentConfigFile,
) -> Result<RemoteAgentConfigFile, TrackError> {
    let host = remote_agent.host.trim().to_owned();
    let user = remote_agent.user.trim().to_owned();
    let workspace_root = remote_agent.workspace_root.trim().to_owned();
    let projects_registry_path = remote_agent.projects_registry_path.trim().to_owned();
    let shell_prelude = canonicalize_optional_multiline_value(remote_agent.shell_prelude);
    let review_follow_up = remote_agent
        .review_follow_up
        .map(|review_follow_up| {
            let main_user = review_follow_up
                .main_user
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            let default_review_prompt =
                canonicalize_optional_multiline_value(review_follow_up.default_review_prompt);

            if review_follow_up.enabled && main_user.is_none() {
                return Err(TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    "Remote review follow-up requires `mainUser` when the feature is enabled.",
                ));
            }

            if !review_follow_up.enabled && main_user.is_none() && default_review_prompt.is_none() {
                return Ok(None);
            }

            Ok(Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: review_follow_up.enabled,
                main_user,
                default_review_prompt,
            }))
        })
        .transpose()?
        .flatten();

    if host.is_empty()
        || user.is_empty()
        || workspace_root.is_empty()
        || projects_registry_path.is_empty()
        || remote_agent.port == 0
    {
        return Err(TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            "Remote agent config requires host, user, workspace root, projects registry path, and a valid SSH port.",
        ));
    }

    Ok(RemoteAgentConfigFile {
        host,
        user,
        port: remote_agent.port,
        workspace_root,
        projects_registry_path,
        preferred_tool: remote_agent.preferred_tool,
        shell_prelude,
        review_follow_up,
    })
}

#[cfg(test)]
mod tests {
    use track_types::types::RemoteAgentPreferredTool;

    use super::{
        canonicalize_optional_multiline_value, canonicalize_remote_agent_config,
        RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile, DEFAULT_REMOTE_AGENT_PORT,
        DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT, DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
    };

    #[test]
    fn canonicalizes_remote_agent_config() {
        let loaded = canonicalize_remote_agent_config(RemoteAgentConfigFile {
            host: " 127.0.0.1 ".to_owned(),
            user: " builder ".to_owned(),
            port: DEFAULT_REMOTE_AGENT_PORT,
            workspace_root: format!(" {} ", DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT),
            projects_registry_path: format!(" {} ", DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH),
            preferred_tool: RemoteAgentPreferredTool::Claude,
            shell_prelude: Some("  export PATH=\"$PATH\"\r\n".to_owned()),
            review_follow_up: Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: true,
                main_user: Some(" octocat ".to_owned()),
                default_review_prompt: Some(" Focus on regressions. \r\n".to_owned()),
            }),
        })
        .expect("remote agent config should canonicalize");

        assert_eq!(loaded.host, "127.0.0.1");
        assert_eq!(loaded.user, "builder");
        assert_eq!(loaded.workspace_root, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT);
        assert_eq!(
            loaded.projects_registry_path,
            DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH
        );
        assert_eq!(
            loaded.shell_prelude.as_deref(),
            Some("export PATH=\"$PATH\"")
        );
        assert_eq!(
            loaded
                .review_follow_up
                .as_ref()
                .and_then(|config| config.main_user.as_deref()),
            Some("octocat")
        );
    }

    #[test]
    fn rejects_enabled_follow_up_without_main_user() {
        let error = canonicalize_remote_agent_config(RemoteAgentConfigFile {
            host: "127.0.0.1".to_owned(),
            user: "builder".to_owned(),
            port: DEFAULT_REMOTE_AGENT_PORT,
            workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
            projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: None,
            review_follow_up: Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: true,
                main_user: None,
                default_review_prompt: None,
            }),
        })
        .expect_err("follow-up config should reject missing main user");

        assert_eq!(
            error.message(),
            "Remote review follow-up requires `mainUser` when the feature is enabled."
        );
    }

    #[test]
    fn drops_empty_multiline_values() {
        assert_eq!(
            canonicalize_optional_multiline_value(Some(" \r\n ".to_owned())),
            None
        );
    }
}
