use std::env;

use track_capture::{LocalTaskParserFactory, TaskCaptureService, TaskParserFactory};
use track_core::api_notify::notify_task_changed;
use track_core::config::ConfigService;
use track_core::errors::{ErrorCode, TrackError};
use track_core::paths::collapse_home_path;
use track_core::project_repository::ProjectRepository;
use track_core::task_repository::FileTaskRepository;
use track_core::terminal_ui::{format_summary, SummaryTone, ValueTone};
use track_core::types::{StoredTask, TaskSource};
use track_core::wizard::{
    run_configure_command, run_configure_command_with_prompter, ConfigureReason, Prompter,
};

fn format_created_task_output(result: &StoredTask) -> String {
    let priority_tone = match result.task.priority.as_str() {
        "high" => ValueTone::PriorityHigh,
        "medium" => ValueTone::PriorityMedium,
        "low" => ValueTone::PriorityLow,
        _ => ValueTone::Plain,
    };
    let status_tone = match result.task.status.as_str() {
        "open" => ValueTone::StatusOpen,
        "closed" => ValueTone::StatusClosed,
        _ => ValueTone::Plain,
    };

    format_summary(
        "Created task",
        SummaryTone::Success,
        &[
            ("Project", result.task.project.clone(), ValueTone::Plain),
            (
                "Priority",
                result.task.priority.as_str().to_owned(),
                priority_tone,
            ),
            (
                "Status",
                result.task.status.as_str().to_owned(),
                status_tone,
            ),
            (
                "File",
                collapse_home_path(&result.file_path),
                ValueTone::Path,
            ),
        ],
    )
}

pub fn run_cli(raw_args: Vec<String>) -> Result<String, TrackError> {
    let config_service = ConfigService::new(None)?;

    if raw_args.is_empty() {
        return run_configure_command(&config_service, ConfigureReason::Manual);
    }

    run_create_command_with_prompter(&raw_args, &config_service, None)
}

pub fn run_create_command_with_prompter(
    argv: &[String],
    config_service: &ConfigService,
    prompter: Option<&mut dyn Prompter>,
) -> Result<String, TrackError> {
    match prompter {
        Some(prompter) => run_create_command_internal(
            argv,
            config_service,
            Some(prompter),
            None,
            &LocalTaskParserFactory,
        ),
        None => {
            run_create_command_internal(argv, config_service, None, None, &LocalTaskParserFactory)
        }
    }
}

fn run_create_command_internal(
    argv: &[String],
    config_service: &ConfigService,
    mut prompter: Option<&mut dyn Prompter>,
    data_dir_override: Option<std::path::PathBuf>,
    task_parser_factory: &dyn TaskParserFactory,
) -> Result<String, TrackError> {
    let raw_text = argv.join(" ").trim().to_owned();
    if raw_text.is_empty() {
        return Err(TrackError::new(
            ErrorCode::EmptyInput,
            "Please provide a task description.",
        ));
    }

    let mut config_setup_output = None;
    let config = match config_service.load_runtime_config() {
        Ok(config) => config,
        Err(error) if error.code == ErrorCode::ConfigNotFound => {
            let output = match prompter.as_deref_mut() {
                Some(prompter) => run_configure_command_with_prompter(
                    config_service,
                    prompter,
                    ConfigureReason::FirstRun,
                )?,
                None => run_configure_command(config_service, ConfigureReason::FirstRun)?,
            };
            config_setup_output = Some(output);
            config_service.load_runtime_config()?
        }
        Err(error) => return Err(error),
    };

    if config.project_roots.is_empty() {
        return Err(TrackError::new(
            ErrorCode::NoProjectRoots,
            "No project roots configured.",
        ));
    }

    let project_repository = ProjectRepository::new(data_dir_override.clone())?;
    let repository = FileTaskRepository::new(data_dir_override)?;
    let capture_service = TaskCaptureService {
        config_service,
        project_repository: &project_repository,
        task_repository: &repository,
        task_parser_factory,
    };
    let stored_task = capture_service.create_task_from_text(&raw_text, Some(TaskSource::Cli))?;
    if let Err(error) = notify_task_changed(&config.api) {
        if env::var("TRACK_DEBUG_API_NOTIFY").ok().as_deref() == Some("1") {
            eprintln!("Skipping API task-change notify: {error}");
        }
    }

    let created_task_output = format_created_task_output(&stored_task);
    match config_setup_output {
        Some(output) => Ok(format!("{output}\n\n{created_task_output}")),
        None => Ok(created_task_output),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;

    use tempfile::TempDir;
    use track_capture::{TaskParser, TaskParserFactory};
    use track_core::config::ConfigService;
    use track_core::errors::TrackError;
    use track_core::project_catalog::ProjectCatalog;
    use track_core::types::{Confidence, ParsedTaskCandidate, Priority, TrackRuntimeConfig};
    use track_core::wizard::Prompter;

    use super::run_create_command_internal;

    struct ScriptedPrompter {
        answers: VecDeque<String>,
    }

    impl ScriptedPrompter {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|value| (*value).to_owned()).collect(),
            }
        }
    }

    impl Prompter for ScriptedPrompter {
        fn ask(&mut self, _prompt: &str) -> Result<String, track_core::errors::TrackError> {
            Ok(self
                .answers
                .pop_front()
                .expect("scripted prompt should have enough answers"))
        }

        fn println(&mut self, _line: &str) {}
    }

    struct StaticTaskParser {
        candidate: ParsedTaskCandidate,
    }

    impl TaskParser for StaticTaskParser {
        fn parse_task(
            &self,
            _raw_text: &str,
            _project_catalog: &ProjectCatalog,
        ) -> Result<ParsedTaskCandidate, TrackError> {
            Ok(self.candidate.clone())
        }
    }

    struct StaticTaskParserFactory {
        candidate: ParsedTaskCandidate,
    }

    impl TaskParserFactory for StaticTaskParserFactory {
        fn create_parser(
            &self,
            _config: &TrackRuntimeConfig,
        ) -> Result<Box<dyn TaskParser + 'static>, TrackError> {
            Ok(Box::new(StaticTaskParser {
                candidate: self.candidate.clone(),
            }))
        }
    }

    #[test]
    fn creates_task_after_first_run_setup() {
        let directory = TempDir::new().expect("tempdir should be created");
        let config_service = ConfigService::new(Some(directory.path().join("config.json")))
            .expect("config service should resolve");
        let project_root = directory.path().join("projects");
        fs::create_dir_all(project_root.join("project-x/.git"))
            .expect("fake git repo should be created");
        let parser_factory = StaticTaskParserFactory {
            candidate: ParsedTaskCandidate {
                project: Some("project-x".to_owned()),
                priority: Priority::High,
                title: "Fix a bug in module A".to_owned(),
                body_markdown: Some("- Inspect `module_a.rs`".to_owned()),
                confidence: Confidence::High,
                reason: None,
            },
        };

        let mut prompter = ScriptedPrompter::new(&[
            "3210",
            project_root.to_string_lossy().as_ref(),
            "proj-x=project-x",
            "",
        ]);

        let output = run_create_command_internal(
            &[
                "proj-x".to_owned(),
                "fix".to_owned(),
                "a".to_owned(),
                "bug".to_owned(),
            ],
            &config_service,
            Some(&mut prompter),
            Some(directory.path().join("issues")),
            &parser_factory,
        )
        .expect("create command should succeed after first-run setup");

        assert!(output.contains("Config created"));
        assert!(output.contains("Created task"));
        assert!(
            directory
                .path()
                .join("issues/project-x/PROJECT.md")
                .exists(),
            "task creation should initialize project metadata alongside the task directory",
        );
    }
}
