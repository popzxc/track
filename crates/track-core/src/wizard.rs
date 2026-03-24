use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal};

use dialoguer::{theme::ColorfulTheme, Input};

use crate::config::{
    ApiConfigFile, ConfigService, LlamaCppConfigFile, RemoteAgentConfigFile, TrackConfigFile,
    DEFAULT_LLAMACPP_MODEL_HF_FILE, DEFAULT_LLAMACPP_MODEL_HF_REPO, DEFAULT_REMOTE_AGENT_PORT,
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT, DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use crate::errors::{ErrorCode, TrackError};
use crate::paths::{
    collapse_home_path, collapse_path_value, get_managed_remote_agent_key_path,
    get_managed_remote_agent_known_hosts_path, resolve_path_from_invocation_dir,
};
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

fn collapse_optional_path_value(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(collapse_path_value)
}

fn format_model_source_display(config: &LlamaCppConfigFile) -> (String, ValueTone) {
    // Prefer the Hugging Face reference when present because that is the
    // active source once both are configured. The local path remains useful as
    // an override or migration fallback, but the repo+file pair is what the
    // downloader will consult first.
    if let (Some(repo), Some(file)) = (
        config.model_hf_repo.as_deref(),
        config.model_hf_file.as_deref(),
    ) {
        return (format!("{repo}#{file}"), ValueTone::Plain);
    }

    match config.model_path.as_deref() {
        Some(path) if !path.is_empty() => (collapse_path_value(path), ValueTone::Path),
        _ => ("not configured".to_owned(), ValueTone::Plain),
    }
}

fn create_default_config_file() -> TrackConfigFile {
    TrackConfigFile {
        project_roots: Vec::new(),
        project_aliases: BTreeMap::new(),
        api: ApiConfigFile::default(),
        llama_cpp: LlamaCppConfigFile {
            model_path: None,
            model_hf_repo: Some(DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned()),
            model_hf_file: Some(DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned()),
            llama_completion_path: None,
        },
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

fn prompt_model_source(
    prompter: &mut dyn Prompter,
    defaults: &LlamaCppConfigFile,
) -> Result<(Option<String>, Option<String>, Option<String>), TrackError> {
    loop {
        let model_path_default = collapse_optional_path_value(defaults.model_path.as_deref());
        let model_hf_repo_default = defaults.model_hf_repo.as_ref().cloned();
        let model_hf_file_default = defaults.model_hf_file.as_ref().cloned();
        let model_path = prompt_with_default(
            prompter,
            "Model file (optional)",
            model_path_default.as_deref(),
            true,
        )?;
        let model_hf_repo = prompt_with_default(
            prompter,
            "HF model repo (optional)",
            model_hf_repo_default.as_deref(),
            true,
        )?;
        let model_hf_file = prompt_with_default(
            prompter,
            "HF model file (optional)",
            model_hf_file_default.as_deref(),
            true,
        )?;

        let model_path = (!model_path.trim().is_empty()).then_some(model_path);
        let model_hf_repo = (!model_hf_repo.trim().is_empty()).then_some(model_hf_repo);
        let model_hf_file = (!model_hf_file.trim().is_empty()).then_some(model_hf_file);
        let has_hf_model = model_hf_repo.is_some() || model_hf_file.is_some();

        if model_path.is_none() && !has_hf_model {
            prompter.println(
                "Please configure either a local model file or both Hugging Face model fields.",
            );
            continue;
        }

        if model_hf_repo.is_some() != model_hf_file.is_some() {
            prompter.println(
                "Please enter both Hugging Face model fields or clear both with `none`.",
            );
            continue;
        }

        return Ok((model_path, model_hf_repo, model_hf_file));
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
        let label = if has_existing_managed_key {
            "SSH private key to import"
        } else {
            "SSH private key to import"
        };
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
    let (model_display, model_tone) = format_model_source_display(&config.llama_cpp);
    let (binary_display, binary_tone) = match config.llama_cpp.llama_completion_path.as_deref() {
        Some(path) if !path.is_empty() => (collapse_path_value(path), ValueTone::Path),
        _ => ("PATH lookup".to_owned(), ValueTone::Plain),
    };
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

    format_summary(
        match reason {
            ConfigureReason::FirstRun => "Config created",
            ConfigureReason::Manual => "Config updated",
        },
        SummaryTone::Success,
        &[
            ("File", collapse_home_path(config_path), ValueTone::Path),
            ("Model", model_display, model_tone),
            ("Binary", binary_display, binary_tone),
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
    )
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

    // The wizard stays linear on purpose: choose the model-source inputs first,
    // then define the filesystem roots that project discovery relies on.
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

    let llama_completion_default = defaults
        .llama_cpp
        .llama_completion_path
        .as_deref()
        .map(collapse_path_value);

    let (model_path, model_hf_repo, model_hf_file) =
        prompt_model_source(prompter, &defaults.llama_cpp)?;
    let llama_completion_path = prompt_with_default(
        prompter,
        "llama-completion binary",
        llama_completion_default.as_deref(),
        true,
    )?;
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

        Some(RemoteAgentConfigFile {
            host,
            user: remote_user,
            port: remote_port,
            workspace_root: remote_workspace_root,
            projects_registry_path: remote_projects_registry_path,
            // TODO: If people start preferring the terminal wizard for remote
            // dispatch setup, add a multiline editor flow here. For now the
            // shell prelude stays web-managed so the wizard preserves an
            // existing value instead of trying to squeeze multiline shell
            // setup into a single-line prompt.
            shell_prelude: existing_remote_agent.and_then(|remote_agent| {
                remote_agent.shell_prelude.clone()
            }),
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
        llama_cpp: LlamaCppConfigFile {
            model_path,
            model_hf_repo,
            model_hf_file,
            llama_completion_path: if llama_completion_path.trim().is_empty() {
                None
            } else {
                Some(llama_completion_path)
            },
        },
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
    use std::collections::VecDeque;

    use tempfile::TempDir;

    use super::{
        parse_project_aliases_input, parse_project_roots_input,
        run_configure_command_with_prompter, ConfigureReason, Prompter,
    };
    use crate::config::ConfigService;

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
        fn ask(&mut self, _prompt: &str) -> Result<String, crate::errors::TrackError> {
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
        let mut prompter = ScriptedPrompter::new(&[
            "",
            "",
            "",
            "~/temp_work/llama.cpp/build/bin/llama-completion",
            "3210",
            "~/work, ~/oss",
            "proj-x=project-x",
            "",
        ]);

        let output =
            run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::FirstRun)
                .expect("config wizard should succeed");

        assert!(output.contains("Config created"));
        let raw = std::fs::read_to_string(service.resolved_path()).expect("config should save");
        assert!(raw.contains("\"llamaCpp\""));
        assert!(raw.contains("\"modelHfRepo\""));
        assert!(raw.contains("\"modelHfFile\""));
        assert!(raw.contains("\"projectRoots\""));
        assert!(raw.contains("\"api\""));
    }
}
