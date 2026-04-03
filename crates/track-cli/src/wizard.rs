use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal};

use dialoguer::{theme::ColorfulTheme, Input};
use track_config::config::{
    ApiConfigFile, ConfigService, LlamaCppConfigFile, RemoteAgentConfigFile,
    RemoteAgentReviewFollowUpConfigFile, TrackConfigFile, DEFAULT_LLAMACPP_MODEL_HF_FILE,
    DEFAULT_LLAMACPP_MODEL_HF_REPO, DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use track_config::paths::{
    collapse_home_path, collapse_path_value, get_managed_remote_agent_key_path,
    get_managed_remote_agent_known_hosts_path, resolve_path_from_invocation_dir,
};
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteAgentPreferredTool;

use crate::terminal_ui::{
    format_note, format_prompt_label, format_summary, SummaryTone, ValueTone,
};

pub const NONE_SENTINEL: &str = "none";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigureReason {
    FirstRun,
    Manual,
}

pub trait Prompter {
    fn ask(&mut self, prompt: &str) -> Result<String, TrackError>;
    fn println(&mut self, line: &str);
}

pub struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn ask(&mut self, prompt: &str) -> Result<String, TrackError> {
        Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::InteractiveRequired,
                    format!("Could not read interactive input: {error}"),
                )
            })
    }

    fn println(&mut self, line: &str) {
        println!("{line}");
    }
}

pub fn parse_project_roots_input(input: &str) -> Vec<String> {
    input
        .split([',', '\n'])
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
}

pub fn parse_project_aliases_input(input: &str) -> Result<BTreeMap<String, String>, TrackError> {
    if input.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    let mut aliases = BTreeMap::new();

    for entry in input
        .split([',', '\n'])
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        let Some((alias, canonical_name)) = entry.split_once('=') else {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                "Project aliases must use alias=canonical-name format.",
            ));
        };

        let alias = alias.trim();
        let canonical_name = canonical_name.trim();
        if alias.is_empty() || canonical_name.is_empty() {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                "Project aliases must use alias=canonical-name format.",
            ));
        }

        aliases.insert(alias.to_owned(), canonical_name.to_owned());
    }

    Ok(aliases)
}

fn format_project_aliases_input(aliases: &BTreeMap<String, String>) -> String {
    aliases
        .iter()
        .map(|(alias, canonical_name)| format!("{alias}={canonical_name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_project_roots_display(roots: &[String]) -> String {
    roots
        .iter()
        .map(|value| collapse_path_value(value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn create_default_config_file() -> TrackConfigFile {
    TrackConfigFile {
        project_roots: Vec::new(),
        project_aliases: BTreeMap::new(),
        api: ApiConfigFile::default(),
        llama_cpp: LlamaCppConfigFile::default(),
        remote_agent: None,
    }
}

fn ensure_interactive_terminal(config_path: &std::path::Path) -> Result<(), TrackError> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Ok(());
    }

    Err(TrackError::new(
        ErrorCode::InteractiveRequired,
        format!(
            "Config setup requires an interactive terminal. Create {} manually or rerun `track` in a terminal.",
            collapse_home_path(config_path)
        ),
    ))
}

fn prompt_with_default(
    prompter: &mut dyn Prompter,
    label: &str,
    default_value: Option<&str>,
    allow_clear: bool,
) -> Result<String, TrackError> {
    let prompt = format_prompt_label(label, default_value.filter(|value| !value.is_empty()));

    let response = prompter.ask(&prompt)?.trim().to_owned();

    if allow_clear && response.eq_ignore_ascii_case(NONE_SENTINEL) {
        return Ok(String::new());
    }

    if response.is_empty() {
        return Ok(default_value.unwrap_or_default().to_owned());
    }

    Ok(response)
}

fn prompt_required_value(
    prompter: &mut dyn Prompter,
    label: &str,
    default_value: Option<&str>,
) -> Result<String, TrackError> {
    loop {
        let response = prompt_with_default(prompter, label, default_value, false)?;
        if !response.trim().is_empty() {
            return Ok(response.trim().to_owned());
        }

        prompter.println("Please enter a value.");
    }
}

fn prompt_project_roots(
    prompter: &mut dyn Prompter,
    defaults: &[String],
) -> Result<Vec<String>, TrackError> {
    loop {
        let response = prompt_with_default(
            prompter,
            "Project roots (comma-separated)",
            Some(&format_project_roots_display(defaults)),
            false,
        )?;

        let project_roots = parse_project_roots_input(&response);
        if !project_roots.is_empty() {
            return Ok(project_roots);
        }

        prompter.println("Please enter at least one project root.");
    }
}

fn prompt_project_aliases(
    prompter: &mut dyn Prompter,
    defaults: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, TrackError> {
    loop {
        let response = prompt_with_default(
            prompter,
            "Project aliases (alias=canonical-name, comma-separated)",
            Some(&format_project_aliases_input(defaults)),
            true,
        )?;

        match parse_project_aliases_input(&response) {
            Ok(aliases) => return Ok(aliases),
            Err(error) => prompter.println(error.message()),
        }
    }
}

fn prompt_api_port(prompter: &mut dyn Prompter, default_port: u16) -> Result<u16, TrackError> {
    loop {
        let response = prompt_with_default(
            prompter,
            "Local API port",
            Some(&default_port.to_string()),
            false,
        )?;

        match response.parse::<u16>() {
            Ok(port) if port > 0 => return Ok(port),
            _ => prompter.println("Please enter a valid TCP port."),
        }
    }
}

fn prompt_remote_agent_host(
    prompter: &mut dyn Prompter,
    default_host: Option<&str>,
) -> Result<Option<String>, TrackError> {
    let response = prompt_with_default(prompter, "Remote agent host", default_host, true)?;
    let trimmed = response.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_owned()))
    }
}

fn prompt_remote_agent_port(
    prompter: &mut dyn Prompter,
    default_port: u16,
) -> Result<u16, TrackError> {
    loop {
        let response = prompt_with_default(
            prompter,
            "Remote SSH port",
            Some(&default_port.to_string()),
            false,
        )?;

        match response.parse::<u16>() {
            Ok(port) if port > 0 => return Ok(port),
            _ => prompter.println("Please enter a valid SSH port."),
        }
    }
}

fn prompt_yes_no(
    prompter: &mut dyn Prompter,
    label: &str,
    default_value: bool,
) -> Result<bool, TrackError> {
    loop {
        let default_display = if default_value { "yes" } else { "no" };
        let response = prompt_with_default(prompter, label, Some(default_display), false)?;

        match response.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" | "true" => return Ok(true),
            "n" | "no" | "false" => return Ok(false),
            _ => prompter.println("Please answer yes or no."),
        }
    }
}

fn managed_remote_agent_key_exists() -> Result<bool, TrackError> {
    Ok(get_managed_remote_agent_key_path()?.exists())
}

fn install_managed_remote_agent_key(source_path: &str) -> Result<(), TrackError> {
    let source_path = resolve_path_from_invocation_dir(source_path)?;
    let managed_key_path = get_managed_remote_agent_key_path()?;
    let known_hosts_path = get_managed_remote_agent_known_hosts_path()?;

    let Some(parent_directory) = managed_key_path.parent() else {
        return Err(TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            "Could not determine the managed remote-agent directory.",
        ));
    };

    fs::create_dir_all(parent_directory).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            format!(
                "Could not create the managed remote-agent directory at {}: {error}",
                collapse_home_path(parent_directory)
            ),
        )
    })?;

    fs::copy(&source_path, &managed_key_path).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            format!(
                "Could not copy the SSH private key from {} to {}: {error}",
                collapse_home_path(&source_path),
                collapse_home_path(&managed_key_path)
            ),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&managed_key_path, fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    format!(
                        "Could not set permissions on the managed SSH private key at {}: {error}",
                        collapse_home_path(&managed_key_path)
                    ),
                )
            },
        )?;
    }

    if !known_hosts_path.exists() {
        fs::write(&known_hosts_path, "").map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                format!(
                    "Could not create the managed known_hosts file at {}: {error}",
                    collapse_home_path(&known_hosts_path)
                ),
            )
        })?;
    }

    Ok(())
}

fn prompt_remote_agent_key_import(
    prompter: &mut dyn Prompter,
    has_existing_managed_key: bool,
) -> Result<(), TrackError> {
    loop {
        let label = "SSH private key to import";
        let response = prompt_with_default(prompter, label, None, false)?;
        let trimmed = response.trim();

        if trimmed.is_empty() && has_existing_managed_key {
            return Ok(());
        }

        if trimmed.is_empty() {
            prompter.println(
                "Please provide a private SSH key path or finish setup later by rerunning `track`.",
            );
            continue;
        }

        return install_managed_remote_agent_key(trimmed);
    }
}

fn format_config_saved_output(
    config: &TrackConfigFile,
    config_path: &std::path::Path,
    reason: ConfigureReason,
) -> String {
    let (remote_agent_display, remote_agent_tone) = match config.remote_agent.as_ref() {
        Some(remote_agent) => (
            format!(
                "{}@{}:{}",
                remote_agent.user, remote_agent.host, remote_agent.port
            ),
            ValueTone::Plain,
        ),
        None => ("disabled".to_owned(), ValueTone::Plain),
    };

    let summary = format_summary(
        match reason {
            ConfigureReason::FirstRun => "Config created",
            ConfigureReason::Manual => "Config updated",
        },
        SummaryTone::Success,
        &[
            ("File", collapse_home_path(config_path), ValueTone::Path),
            (
                "Project roots",
                format!("{} configured", config.project_roots.len()),
                ValueTone::Plain,
            ),
            (
                "Aliases",
                format!("{} configured", config.project_aliases.len()),
                ValueTone::Plain,
            ),
            ("API port", config.api.port.to_string(), ValueTone::Plain),
            ("Remote", remote_agent_display, remote_agent_tone),
        ],
    );

    let preserved_model_override_fields = preserved_model_override_fields(&config.llama_cpp);
    if preserved_model_override_fields.is_empty() {
        return summary;
    }

    format!(
        "{summary}\n\n{}",
        format_note(
            "Advanced",
            &format!(
                "The following fields are set in {} but are not managed by the wizard: {}. Edit the file directly if you need to change them.",
                collapse_home_path(config_path),
                preserved_model_override_fields.join(", "),
            ),
        )
    )
}

fn preserved_model_override_fields(config: &LlamaCppConfigFile) -> Vec<&'static str> {
    let mut fields = Vec::new();

    if config.model_path.is_some() {
        fields.push("llamaCpp.modelPath");
    }

    if let (Some(repo), Some(file)) = (
        config.model_hf_repo.as_deref(),
        config.model_hf_file.as_deref(),
    ) {
        let uses_builtin_default =
            repo == DEFAULT_LLAMACPP_MODEL_HF_REPO && file == DEFAULT_LLAMACPP_MODEL_HF_FILE;
        if !uses_builtin_default {
            fields.push("llamaCpp.modelHfRepo");
            fields.push("llamaCpp.modelHfFile");
        }
    }

    fields
}

pub fn run_configure_command(
    config_service: &ConfigService,
    reason: ConfigureReason,
) -> Result<String, TrackError> {
    ensure_interactive_terminal(config_service.resolved_path())?;
    let mut prompter = TerminalPrompter;
    run_configure_command_with_prompter(config_service, &mut prompter, reason)
}

pub fn run_configure_command_with_prompter(
    config_service: &ConfigService,
    prompter: &mut dyn Prompter,
    reason: ConfigureReason,
) -> Result<String, TrackError> {
    let existing_config = match config_service.load_config_file() {
        Ok(config) => Some(config),
        Err(error) if error.code == ErrorCode::ConfigNotFound => None,
        Err(error) => return Err(error),
    };
    let defaults = existing_config.unwrap_or_else(create_default_config_file);

    // The wizard stays linear on purpose: establish the local filesystem and
    // API settings first, then optionally layer in remote-agent details.
    let intro = match reason {
        ConfigureReason::FirstRun => format_summary(
            "Config setup",
            SummaryTone::Info,
            &[(
                "File",
                collapse_home_path(config_service.resolved_path()),
                ValueTone::Path,
            )],
        ),
        ConfigureReason::Manual => format_summary(
            "Config editor",
            SummaryTone::Info,
            &[(
                "File",
                collapse_home_path(config_service.resolved_path()),
                ValueTone::Path,
            )],
        ),
    };
    prompter.println(&intro);
    prompter.println(&format_note("Enter", "keep current values"));
    prompter.println(&format_note(NONE_SENTINEL, "clear optional values"));

    let api_port = prompt_api_port(prompter, defaults.api.port)?;
    let project_roots = prompt_project_roots(prompter, &defaults.project_roots)?;
    let project_aliases = prompt_project_aliases(prompter, &defaults.project_aliases)?;
    let remote_agent_host = prompt_remote_agent_host(
        prompter,
        defaults
            .remote_agent
            .as_ref()
            .map(|remote_agent| remote_agent.host.as_str()),
    )?;
    let remote_agent = if let Some(host) = remote_agent_host {
        let existing_remote_agent = defaults.remote_agent.as_ref();
        let remote_user = prompt_required_value(
            prompter,
            "Remote agent user",
            existing_remote_agent.map(|remote_agent| remote_agent.user.as_str()),
        )?;
        let remote_port = prompt_remote_agent_port(
            prompter,
            existing_remote_agent
                .map(|remote_agent| remote_agent.port)
                .unwrap_or(DEFAULT_REMOTE_AGENT_PORT),
        )?;
        let remote_workspace_root = prompt_required_value(
            prompter,
            "Remote workspace root",
            existing_remote_agent
                .map(|remote_agent| remote_agent.workspace_root.as_str())
                .or(Some(DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT)),
        )?;
        let remote_projects_registry_path = prompt_required_value(
            prompter,
            "Remote projects registry path",
            existing_remote_agent
                .map(|remote_agent| remote_agent.projects_registry_path.as_str())
                .or(Some(DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH)),
        )?;
        prompt_remote_agent_key_import(prompter, managed_remote_agent_key_exists()?)?;
        let existing_review_follow_up =
            existing_remote_agent.and_then(|remote_agent| remote_agent.review_follow_up.as_ref());
        let review_follow_up_enabled = prompt_yes_no(
            prompter,
            "Enable automatic GitHub review follow-ups",
            existing_review_follow_up
                .map(|review_follow_up| review_follow_up.enabled)
                .unwrap_or(false),
        )?;

        // We intentionally remember the last configured reviewer when the
        // feature is toggled off. That keeps the wizard fast to re-enable on a
        // later run instead of forcing users back through the web UI.
        // TODO: If users ask to clear the remembered GitHub user from the
        // wizard, add an explicit prompt for that instead of overloading the
        // yes/no flow here.
        let review_follow_up = if review_follow_up_enabled {
            let main_user = prompt_required_value(
                prompter,
                "GitHub user for automatic follow-ups",
                existing_review_follow_up
                    .and_then(|review_follow_up| review_follow_up.main_user.as_deref()),
            )?;

            Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: true,
                main_user: Some(main_user),
                default_review_prompt: existing_review_follow_up
                    .and_then(|review_follow_up| review_follow_up.default_review_prompt.clone()),
            })
        } else {
            existing_review_follow_up
                .and_then(|review_follow_up| review_follow_up.main_user.clone())
                .map(|main_user| RemoteAgentReviewFollowUpConfigFile {
                    enabled: false,
                    main_user: Some(main_user),
                    default_review_prompt: existing_review_follow_up.and_then(|review_follow_up| {
                        review_follow_up.default_review_prompt.clone()
                    }),
                })
        };

        Some(RemoteAgentConfigFile {
            host,
            user: remote_user,
            port: remote_port,
            workspace_root: remote_workspace_root,
            projects_registry_path: remote_projects_registry_path,
            // The web UI owns the preferred runner choice today, so the wizard
            // preserves any existing selection instead of introducing a second
            // place where users have to keep the same setting in sync.
            preferred_tool: existing_remote_agent
                .map(|remote_agent| remote_agent.preferred_tool)
                .unwrap_or(RemoteAgentPreferredTool::Codex),
            // TODO: If people start preferring the terminal wizard for remote
            // dispatch setup, add a multiline editor flow here. For now the
            // shell prelude stays web-managed so the wizard preserves an
            // existing value instead of trying to squeeze multiline shell
            // setup into a single-line prompt.
            shell_prelude: existing_remote_agent
                .and_then(|remote_agent| remote_agent.shell_prelude.clone()),
            review_follow_up,
        })
    } else {
        // TODO: Consider removing the managed SSH key when remote dispatch is
        // disabled explicitly. We leave it in place for now so a temporary
        // config change does not silently destroy a secret the user imported on
        // purpose.
        None
    };

    let config = TrackConfigFile {
        project_roots,
        project_aliases,
        api: ApiConfigFile { port: api_port },
        llama_cpp: defaults.llama_cpp.clone(),
        remote_agent,
    };

    config_service.save_config_file(&config)?;
    Ok(format_config_saved_output(
        &config,
        config_service.resolved_path(),
        reason,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use tempfile::TempDir;

    use super::{
        parse_project_aliases_input, parse_project_roots_input,
        run_configure_command_with_prompter, ConfigureReason, Prompter,
    };
    use crate::test_support::{set_env_var, track_data_env_lock};
    use track_config::config::{
        ConfigService, LlamaCppConfigFile, RemoteAgentReviewFollowUpConfigFile, TrackConfigFile,
        DEFAULT_LLAMACPP_MODEL_HF_FILE, DEFAULT_LLAMACPP_MODEL_HF_REPO,
    };
    use track_types::errors::TrackError;

    struct ScriptedPrompter {
        answers: VecDeque<String>,
        lines: Vec<String>,
    }

    impl ScriptedPrompter {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|value| (*value).to_owned()).collect(),
                lines: Vec::new(),
            }
        }
    }

    impl Prompter for ScriptedPrompter {
        fn ask(&mut self, _prompt: &str) -> Result<String, TrackError> {
            Ok(self
                .answers
                .pop_front()
                .expect("scripted prompt should have enough answers"))
        }

        fn println(&mut self, line: &str) {
            self.lines.push(line.to_owned());
        }
    }

    fn temp_config_service() -> (TempDir, ConfigService) {
        let directory = TempDir::new().expect("tempdir should be created");
        let service = ConfigService::new(Some(directory.path().join("config.json")))
            .expect("config service should resolve");
        (directory, service)
    }

    #[test]
    fn parses_project_roots() {
        assert_eq!(
            parse_project_roots_input("~/work, ~/oss\n~/lab"),
            vec!["~/work", "~/oss", "~/lab"]
        );
    }

    #[test]
    fn parses_project_aliases() {
        let aliases = parse_project_aliases_input("proj-x=project-x, infra=platform")
            .expect("aliases should parse");

        assert_eq!(aliases.get("proj-x"), Some(&"project-x".to_owned()));
        assert_eq!(aliases.get("infra"), Some(&"platform".to_owned()));
    }

    #[test]
    fn writes_first_run_config() {
        let (_directory, service) = temp_config_service();
        let mut prompter =
            ScriptedPrompter::new(&["3210", "~/work, ~/oss", "proj-x=project-x", ""]);

        let output =
            run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::FirstRun)
                .expect("config wizard should succeed");

        assert!(output.contains("Config created"));
        let raw = std::fs::read_to_string(service.resolved_path()).expect("config should save");
        assert!(!raw.contains("\"llamaCpp\""));
        assert!(raw.contains("\"projectRoots\""));
        assert!(raw.contains("\"api\""));
    }

    #[test]
    fn mentions_preserved_manual_model_overrides() {
        let (_directory, service) = temp_config_service();
        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["~/work".to_owned()],
                project_aliases: BTreeMap::new(),
                api: track_config::config::ApiConfigFile::default(),
                llama_cpp: LlamaCppConfigFile {
                    model_path: Some("~/.models/custom.gguf".to_owned()),
                    model_hf_repo: None,
                    model_hf_file: None,
                },
                remote_agent: None,
            })
            .expect("seed config should save");

        let mut prompter = ScriptedPrompter::new(&["", "", "", ""]);
        let output =
            run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::Manual)
                .expect("config wizard should succeed");

        assert!(output.contains("llamaCpp.modelPath"));
    }

    #[test]
    fn does_not_call_out_builtin_hugging_face_defaults() {
        let (_directory, service) = temp_config_service();
        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["~/work".to_owned()],
                project_aliases: BTreeMap::new(),
                api: track_config::config::ApiConfigFile::default(),
                llama_cpp: LlamaCppConfigFile {
                    model_path: None,
                    model_hf_repo: Some(DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned()),
                    model_hf_file: Some(DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned()),
                },
                remote_agent: None,
            })
            .expect("seed config should save");

        let mut prompter = ScriptedPrompter::new(&["", "", "", ""]);
        let output =
            run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::Manual)
                .expect("config wizard should succeed");

        assert!(!output.contains("llamaCpp.modelHfRepo"));
        assert!(!output.contains("llamaCpp.modelHfFile"));
    }

    #[test]
    fn writes_remote_review_follow_up_from_wizard() {
        let (_directory, service) = temp_config_service();
        let _track_data_dir_guard = track_data_env_lock()
            .lock()
            .expect("track data dir lock should not be poisoned");
        let data_dir = service
            .resolved_path()
            .parent()
            .expect("config path should have a parent")
            .join("track-data")
            .join("issues");
        let _track_data_dir = set_env_var("TRACK_DATA_DIR", &data_dir);

        let ssh_key_source = data_dir
            .parent()
            .expect("data dir should have a parent")
            .join("id_ed25519.source");
        std::fs::create_dir_all(
            ssh_key_source
                .parent()
                .expect("SSH key source should have a parent"),
        )
        .expect("SSH key source parent should be created");
        std::fs::write(&ssh_key_source, "not-a-real-private-key")
            .expect("SSH key source should be written");

        let ssh_key_source = ssh_key_source.to_string_lossy().into_owned();
        let mut prompter = ScriptedPrompter::new(&[
            "3210",
            "~/work",
            "",
            "builder.example.com",
            "codex",
            "2222",
            "/srv/track",
            "/srv/track/projects.json",
            &ssh_key_source,
            "yes",
            "octocat",
        ]);

        run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::FirstRun)
            .expect("config wizard should succeed");

        let saved = service
            .load_config_file()
            .expect("saved config should load successfully");
        let remote_agent = saved
            .remote_agent
            .expect("remote agent config should be present");

        assert_eq!(
            remote_agent.review_follow_up,
            Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: true,
                main_user: Some("octocat".to_owned()),
                default_review_prompt: None,
            })
        );
    }
}
