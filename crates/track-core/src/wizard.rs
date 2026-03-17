use std::collections::BTreeMap;
use std::io::{self, IsTerminal};

use dialoguer::{theme::ColorfulTheme, Input};

use crate::config::{ConfigService, LlamaCppConfigFile, TrackConfigFile};
use crate::errors::{ErrorCode, TrackError};
use crate::paths::{collapse_home_path, collapse_path_value};
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
        llama_cpp: LlamaCppConfigFile {
            model_path: String::new(),
            llama_completion_path: None,
        },
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

fn format_config_saved_output(
    config: &TrackConfigFile,
    config_path: &std::path::Path,
    reason: ConfigureReason,
) -> String {
    let (binary_display, binary_tone) = match config.llama_cpp.llama_completion_path.as_deref() {
        Some(path) if !path.is_empty() => (collapse_path_value(path), ValueTone::Path),
        _ => ("PATH lookup".to_owned(), ValueTone::Plain),
    };

    format_summary(
        match reason {
            ConfigureReason::FirstRun => "Config created",
            ConfigureReason::Manual => "Config updated",
        },
        SummaryTone::Success,
        &[
            ("File", collapse_home_path(config_path), ValueTone::Path),
            (
                "Model",
                collapse_path_value(&config.llama_cpp.model_path),
                ValueTone::Path,
            ),
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

    // The wizard stays linear on purpose: choose the local model inputs first,
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

    let model_path_default = collapse_path_value(&defaults.llama_cpp.model_path);
    let llama_completion_default = defaults
        .llama_cpp
        .llama_completion_path
        .as_deref()
        .map(collapse_path_value);

    let model_path = prompt_required_value(prompter, "Model file", Some(&model_path_default))?;
    let llama_completion_path = prompt_with_default(
        prompter,
        "llama-completion binary",
        llama_completion_default.as_deref(),
        true,
    )?;
    let project_roots = prompt_project_roots(prompter, &defaults.project_roots)?;
    let project_aliases = prompt_project_aliases(prompter, &defaults.project_aliases)?;

    let config = TrackConfigFile {
        project_roots,
        project_aliases,
        llama_cpp: LlamaCppConfigFile {
            model_path,
            llama_completion_path: if llama_completion_path.trim().is_empty() {
                None
            } else {
                Some(llama_completion_path)
            },
        },
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
            "~/.models/parser.gguf",
            "~/temp_work/llama.cpp/build/bin/llama-completion",
            "~/work, ~/oss",
            "proj-x=project-x",
        ]);

        let output =
            run_configure_command_with_prompter(&service, &mut prompter, ConfigureReason::FirstRun)
                .expect("config wizard should succeed");

        assert!(output.contains("Config created"));
        let raw = std::fs::read_to_string(service.resolved_path()).expect("config should save");
        assert!(raw.contains("\"llamaCpp\""));
        assert!(raw.contains("\"projectRoots\""));
    }
}
