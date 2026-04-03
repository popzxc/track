use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use track_capture::{build_task_create_input_from_text, LocalTaskParserFactory, TaskParserFactory};
use track_config::config::{
    DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use track_config::paths::collapse_home_path;
use track_projects::project_catalog::{ProjectCatalog, ProjectInfo};
use track_projects::project_metadata::{infer_project_metadata, ProjectRecord};
use track_types::errors::{ErrorCode, TrackError};
use track_types::migration::{MigrationImportSummary, MigrationState, MigrationStatus};
use track_types::path_component::validate_single_normal_path_component;
use track_types::types::{Task, TaskSource};

use crate::backend_client::{
    ConfigureRemoteAgentRequest, ConfigureRemoteAgentReviewFollowUpRequest, HttpTrackBackend,
    TrackBackend,
};
use crate::cli_config::{CliConfigFile, CliConfigService, ConfigureOptions, LoadedCliConfig};
use crate::terminal_ui::{format_note, format_summary, SummaryTone, ValueTone};

#[derive(Debug)]
enum CliInvocation {
    Capture(Vec<String>),
    Configure(ConfigureArgs),
    Migrate(MigrateCommand),
    ProjectRegister(ProjectRegisterArgs),
    RemoteAgentConfigure(RemoteAgentConfigureArgs),
}

#[derive(Debug, Parser)]
#[command(
    name = "track",
    about = "Capture tasks through the track backend.",
    version = crate::build_info::CLI_VERSION_TEXT
)]
struct CommandLine {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Configure(ConfigureArgs),
    #[command(subcommand)]
    Migrate(MigrateCommand),
    #[command(subcommand)]
    Project(ProjectCommand),
    #[command(subcommand)]
    RemoteAgent(RemoteAgentCommand),
}

#[derive(Debug, Clone, Default, Args)]
pub struct ConfigureArgs {
    #[arg(long = "backend-url")]
    backend_url: Option<String>,
    #[arg(long = "model-path")]
    model_path: Option<String>,
    #[arg(long = "model-hf-repo", requires = "model_hf_file")]
    model_hf_repo: Option<String>,
    #[arg(long = "model-hf-file", requires = "model_hf_repo")]
    model_hf_file: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MigrateCommand {
    Status,
    Import,
}

#[derive(Debug, Clone, Subcommand)]
enum ProjectCommand {
    Register(ProjectRegisterArgs),
}

#[derive(Debug, Clone, Subcommand)]
enum RemoteAgentCommand {
    Configure(RemoteAgentConfigureArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ProjectRegisterArgs {
    path: Option<PathBuf>,
    #[arg(long = "alias")]
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct RemoteAgentConfigureArgs {
    #[arg(long)]
    host: String,
    #[arg(long)]
    user: String,
    #[arg(long, default_value_t = DEFAULT_REMOTE_AGENT_PORT)]
    port: u16,
    #[arg(long = "workspace-root", default_value = DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT)]
    workspace_root: String,
    #[arg(
        long = "projects-registry-path",
        default_value = DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH
    )]
    projects_registry_path: String,
    #[arg(long = "identity-file")]
    identity_file: PathBuf,
    #[arg(long = "known-hosts-file")]
    known_hosts_file: Option<PathBuf>,
    #[arg(long = "shell-prelude", conflicts_with = "shell_prelude_file")]
    shell_prelude: Option<String>,
    #[arg(long = "shell-prelude-file", conflicts_with = "shell_prelude")]
    shell_prelude_file: Option<PathBuf>,
    #[arg(long = "enable-review-follow-up", default_value_t = false)]
    enable_review_follow_up: bool,
    #[arg(long = "main-user")]
    main_user: Option<String>,
    #[arg(
        long = "default-review-prompt",
        conflicts_with = "default_review_prompt_file"
    )]
    default_review_prompt: Option<String>,
    #[arg(
        long = "default-review-prompt-file",
        conflicts_with = "default_review_prompt"
    )]
    default_review_prompt_file: Option<PathBuf>,
}

pub fn run_from_os_args(raw_args: Vec<OsString>) -> Result<String, TrackError> {
    let invocation = parse_invocation(raw_args)?;
    let config_service = CliConfigService::new(None, None)?;

    match invocation {
        CliInvocation::Capture(raw_text) => {
            let loaded = config_service.load_or_initialize()?;
            let backend = HttpTrackBackend::new(&loaded.runtime.backend_base_url);
            run_capture_command_internal(
                &raw_text,
                &config_service,
                &loaded,
                &backend,
                &LocalTaskParserFactory,
            )
        }
        CliInvocation::Configure(args) => run_configure_command(&config_service, args),
        CliInvocation::Migrate(command) => {
            let loaded = config_service.load_or_initialize()?;
            let backend = HttpTrackBackend::new(&loaded.runtime.backend_base_url);
            run_migration_command(&config_service, &loaded, &backend, command)
        }
        CliInvocation::ProjectRegister(args) => {
            let loaded = config_service.load_or_initialize()?;
            let backend = HttpTrackBackend::new(&loaded.runtime.backend_base_url);
            run_project_register_command_internal(&config_service, &loaded, &backend, args)
        }
        CliInvocation::RemoteAgentConfigure(args) => {
            let loaded = config_service.load_or_initialize()?;
            let backend = HttpTrackBackend::new(&loaded.runtime.backend_base_url);
            run_remote_agent_configure_command_internal(&config_service, &loaded, &backend, args)
        }
    }
}

fn parse_invocation(raw_args: Vec<OsString>) -> Result<CliInvocation, TrackError> {
    if raw_args.len() <= 1 {
        return Ok(CliInvocation::Configure(ConfigureArgs::default()));
    }

    let first_argument = raw_args[1].to_string_lossy();
    if matches!(
        first_argument.as_ref(),
        "--help" | "-h" | "--version" | "-V"
    ) {
        CommandLine::parse_from(raw_args);
        unreachable!("clap exits after rendering help or version output");
    }
    if matches!(
        first_argument.as_ref(),
        "configure" | "migrate" | "project" | "remote-agent"
    ) {
        let parsed = CommandLine::try_parse_from(raw_args)
            .map_err(|error| TrackError::new(ErrorCode::InvalidConfigInput, error.to_string()))?;
        return Ok(match parsed.command {
            Command::Configure(args) => CliInvocation::Configure(args),
            Command::Migrate(command) => CliInvocation::Migrate(command),
            Command::Project(ProjectCommand::Register(args)) => {
                CliInvocation::ProjectRegister(args)
            }
            Command::RemoteAgent(RemoteAgentCommand::Configure(args)) => {
                CliInvocation::RemoteAgentConfigure(args)
            }
        });
    }

    Ok(CliInvocation::Capture(
        raw_args
            .into_iter()
            .skip(1)
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
    ))
}

fn run_capture_command_internal(
    argv: &[String],
    config_service: &CliConfigService,
    loaded_config: &LoadedCliConfig,
    backend: &dyn TrackBackend,
    task_parser_factory: &dyn TaskParserFactory,
) -> Result<String, TrackError> {
    let raw_text = argv.join(" ").trim().to_owned();
    if raw_text.is_empty() {
        return Err(TrackError::new(
            ErrorCode::EmptyInput,
            "Please provide a task description.",
        ));
    }

    let projects = backend
        .fetch_projects()
        .map_err(enrich_backend_error_for_cli)?;
    if projects.is_empty() {
        return Err(TrackError::new(
            ErrorCode::NoProjectsDiscovered,
            "No registered projects were found on the backend yet. Run `track project register` from a local checkout first.",
        ));
    }

    let project_catalog = project_catalog_from_backend(&projects);
    let task_input = build_task_create_input_from_text(
        &raw_text,
        &project_catalog,
        &loaded_config.runtime.capture_runtime,
        Some(TaskSource::Cli),
        task_parser_factory,
    )?;
    let created_task = backend
        .create_task(&task_input)
        .map_err(enrich_backend_error_for_cli)?;

    render_output_with_config_notes(
        config_service,
        loaded_config,
        format_created_task_output(&created_task),
    )
}

fn run_configure_command(
    config_service: &CliConfigService,
    args: ConfigureArgs,
) -> Result<String, TrackError> {
    let configured = config_service.configure(ConfigureOptions {
        backend_base_url: args.backend_url,
        model_path: args.model_path,
        model_hf_repo: args.model_hf_repo,
        model_hf_file: args.model_hf_file,
    })?;
    let backend_base_url = configured.backend_base_url.clone();
    let model_description = describe_model(&configured);

    Ok(format_summary(
        "Configured CLI",
        SummaryTone::Success,
        &[
            ("Backend", backend_base_url, ValueTone::Path),
            ("Model", model_description, ValueTone::Plain),
            (
                "Config",
                collapse_home_path(config_service.resolved_path()),
                ValueTone::Path,
            ),
        ],
    ))
}

fn run_migration_command(
    config_service: &CliConfigService,
    loaded_config: &LoadedCliConfig,
    backend: &dyn TrackBackend,
    command: MigrateCommand,
) -> Result<String, TrackError> {
    let rendered = match command {
        MigrateCommand::Status => {
            let migration = backend.migration_status()?;
            format_migration_status(&migration)
        }
        MigrateCommand::Import => {
            let summary = backend.import_legacy_data()?;
            format_migration_import_summary(&summary)
        }
    };

    render_output_with_config_notes(config_service, loaded_config, rendered)
}

fn run_project_register_command_internal(
    config_service: &CliConfigService,
    loaded_config: &LoadedCliConfig,
    backend: &dyn TrackBackend,
    args: ProjectRegisterArgs,
) -> Result<String, TrackError> {
    let checkout_path = resolve_register_path(args.path.as_deref())?;
    if !checkout_path.join(".git").exists() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Project registration expects a git checkout, but {} does not contain a `.git` directory.",
                collapse_home_path(&checkout_path)
            ),
        ));
    }

    let canonical_name = validate_single_normal_path_component(
        checkout_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default(),
        "Project canonical name",
        ErrorCode::InvalidPathComponent,
    )?;
    let aliases = canonicalize_aliases(args.aliases, &canonical_name)?;
    let project_info = ProjectInfo {
        canonical_name: canonical_name.clone(),
        path: checkout_path.clone(),
        aliases: aliases.clone(),
    };
    let metadata = infer_project_metadata(&project_info);
    let project = backend
        .register_project(&canonical_name, aliases, metadata)
        .map_err(enrich_backend_error_for_cli)?;

    render_output_with_config_notes(
        config_service,
        loaded_config,
        format_registered_project_output(&project, &checkout_path),
    )
}

fn run_remote_agent_configure_command_internal(
    config_service: &CliConfigService,
    loaded_config: &LoadedCliConfig,
    backend: &dyn TrackBackend,
    args: RemoteAgentConfigureArgs,
) -> Result<String, TrackError> {
    let identity_file = resolve_local_file(&args.identity_file, "SSH private key")?;
    let ssh_private_key = fs::read_to_string(&identity_file).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            format!(
                "Could not read the SSH private key at {}: {error}",
                collapse_home_path(&identity_file)
            ),
        )
    })?;
    let known_hosts = match args.known_hosts_file.as_deref() {
        Some(path) => Some(read_optional_text_file(path, "known_hosts")?),
        None => None,
    };
    let shell_prelude = match (args.shell_prelude, args.shell_prelude_file.as_deref()) {
        (Some(shell_prelude), None) => Some(shell_prelude),
        (None, Some(path)) => Some(read_optional_text_file(path, "shell prelude")?),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap enforces shell prelude exclusivity"),
    };
    let default_review_prompt = match (
        args.default_review_prompt,
        args.default_review_prompt_file.as_deref(),
    ) {
        (Some(prompt), None) => Some(prompt),
        (None, Some(path)) => Some(read_optional_text_file(path, "default review prompt")?),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap enforces review prompt exclusivity"),
    };
    let review_follow_up = if args.enable_review_follow_up
        || args.main_user.is_some()
        || default_review_prompt.is_some()
    {
        Some(ConfigureRemoteAgentReviewFollowUpRequest {
            enabled: args.enable_review_follow_up,
            main_user: args.main_user,
            default_review_prompt,
        })
    } else {
        None
    };

    let configured = backend
        .configure_remote_agent(&ConfigureRemoteAgentRequest {
            host: args.host,
            user: args.user,
            port: args.port,
            workspace_root: args.workspace_root,
            projects_registry_path: args.projects_registry_path,
            shell_prelude,
            review_follow_up,
            ssh_private_key,
            known_hosts,
        })
        .map_err(enrich_backend_error_for_cli)?;

    render_output_with_config_notes(
        config_service,
        loaded_config,
        format_remote_agent_configured_output(&configured, &identity_file),
    )
}

fn format_created_task_output(task: &Task) -> String {
    let priority_tone = match task.priority.as_str() {
        "high" => ValueTone::PriorityHigh,
        "medium" => ValueTone::PriorityMedium,
        "low" => ValueTone::PriorityLow,
        _ => ValueTone::Plain,
    };
    let status_tone = match task.status.as_str() {
        "open" => ValueTone::StatusOpen,
        "closed" => ValueTone::StatusClosed,
        _ => ValueTone::Plain,
    };

    format_summary(
        "Created task",
        SummaryTone::Success,
        &[
            ("Project", task.project.clone(), ValueTone::Plain),
            ("Priority", task.priority.as_str().to_owned(), priority_tone),
            ("Status", task.status.as_str().to_owned(), status_tone),
            ("ID", task.id.clone(), ValueTone::Path),
        ],
    )
}

fn format_registered_project_output(project: &ProjectRecord, checkout_path: &Path) -> String {
    let aliases = if project.aliases.is_empty() {
        "(none)".to_owned()
    } else {
        project.aliases.join(", ")
    };

    format_summary(
        "Registered project",
        SummaryTone::Success,
        &[
            ("Project", project.canonical_name.clone(), ValueTone::Plain),
            ("Aliases", aliases, ValueTone::Plain),
            (
                "Checkout",
                collapse_home_path(checkout_path),
                ValueTone::Path,
            ),
            ("Repo", project.metadata.repo_url.clone(), ValueTone::Path),
        ],
    )
}

fn format_remote_agent_configured_output(
    configured: &crate::backend_client::RemoteAgentSettingsResponse,
    identity_file: &Path,
) -> String {
    format_summary(
        "Configured remote agent",
        SummaryTone::Success,
        &[
            (
                "State",
                if configured.configured {
                    "configured".to_owned()
                } else {
                    "pending".to_owned()
                },
                ValueTone::Plain,
            ),
            (
                "Host",
                configured
                    .host
                    .clone()
                    .unwrap_or_else(|| "(unknown)".to_owned()),
                ValueTone::Plain,
            ),
            (
                "User",
                configured
                    .user
                    .clone()
                    .unwrap_or_else(|| "(unknown)".to_owned()),
                ValueTone::Plain,
            ),
            (
                "Port",
                configured
                    .port
                    .map(|port| port.to_string())
                    .unwrap_or_else(|| "(unknown)".to_owned()),
                ValueTone::Plain,
            ),
            ("Key", collapse_home_path(identity_file), ValueTone::Path),
        ],
    )
}

fn format_migration_status(status: &MigrationStatus) -> String {
    let title = if status.requires_migration {
        "Migration required"
    } else {
        "Migration status"
    };
    let tone = if status.requires_migration {
        SummaryTone::Info
    } else {
        SummaryTone::Success
    };
    let state = match status.state {
        MigrationState::Ready => "ready",
        MigrationState::ImportRequired => "import_required",
        MigrationState::Imported => "imported",
        MigrationState::Skipped => "skipped",
    };
    let skipped_count = status.skipped_records.len().to_string();
    let cleanup_count = status.cleanup_candidates.len().to_string();
    let mut rendered = format_summary(
        title,
        tone,
        &[
            ("State", state.to_owned(), ValueTone::Plain),
            (
                "Projects",
                status.summary.projects_found.to_string(),
                ValueTone::Plain,
            ),
            (
                "Tasks",
                status.summary.tasks_found.to_string(),
                ValueTone::Plain,
            ),
            (
                "Reviews",
                status.summary.reviews_found.to_string(),
                ValueTone::Plain,
            ),
            ("Skipped", skipped_count, ValueTone::Plain),
            ("Cleanup", cleanup_count, ValueTone::Plain),
        ],
    );

    if status.requires_migration {
        rendered.push('\n');
        rendered.push_str(&format_note(
            "Next",
            "Run `track migrate import` to copy legacy data into the SQLite backend.",
        ));
    }

    rendered
}

fn format_migration_import_summary(summary: &MigrationImportSummary) -> String {
    let mut rendered = format_summary(
        "Imported legacy data",
        SummaryTone::Success,
        &[
            (
                "Projects",
                summary.imported_projects.to_string(),
                ValueTone::Plain,
            ),
            (
                "Aliases",
                summary.imported_aliases.to_string(),
                ValueTone::Plain,
            ),
            (
                "Tasks",
                summary.imported_tasks.to_string(),
                ValueTone::Plain,
            ),
            (
                "Reviews",
                summary.imported_reviews.to_string(),
                ValueTone::Plain,
            ),
            (
                "Secrets",
                summary.copied_secret_files.len().to_string(),
                ValueTone::Plain,
            ),
            (
                "Skipped",
                summary.skipped_records.len().to_string(),
                ValueTone::Plain,
            ),
        ],
    );
    if !summary.cleanup_candidates.is_empty() {
        rendered.push('\n');
        rendered.push_str(&format_note(
            "Next",
            "Run `track configure` to materialize `~/.config/track/cli.json` before removing legacy config.",
        ));
        rendered.push('\n');
        rendered.push_str(&format_note(
            "Install",
            "From your `airbender-platform` checkout, run `cargo install --path crates/cargo-airbender --force`.",
        ));
        rendered.push('\n');
        rendered.push_str(&format_note(
            "Keep",
            "Preserve `~/.track/models` if you use local capture.",
        ));
        for candidate in &summary.cleanup_candidates {
            rendered.push('\n');
            rendered.push_str(&format_note(
                "Remove",
                &migration_cleanup_command(&candidate.path),
            ));
        }
    }

    rendered
}

fn migration_cleanup_command(path: &str) -> String {
    if path.ends_with(".json") {
        format!("rm -f {path}")
    } else {
        format!("rm -rf {path}")
    }
}

fn render_output_with_config_notes(
    config_service: &CliConfigService,
    loaded_config: &LoadedCliConfig,
    main_output: String,
) -> Result<String, TrackError> {
    let mut notes = Vec::new();
    if loaded_config.migrated_from_legacy {
        notes.push(format_note(
            "Migrated",
            "CLI config was copied from the legacy shared config.",
        ));
    } else if loaded_config.created_default_config {
        notes.push(format_note(
            "Config",
            &format!(
                "Created a default CLI config at {}.",
                collapse_home_path(config_service.resolved_path())
            ),
        ));
    }

    if notes.is_empty() {
        Ok(main_output)
    } else {
        Ok(format!("{}\n\n{}", notes.join("\n"), main_output))
    }
}

fn describe_model(config: &CliConfigFile) -> String {
    if let Some(model_path) = config.llama_cpp.model_path.as_deref() {
        return model_path.to_owned();
    }
    if let (Some(repo), Some(file)) = (
        config.llama_cpp.model_hf_repo.as_deref(),
        config.llama_cpp.model_hf_file.as_deref(),
    ) {
        return format!("{repo}/{file}");
    }

    "builtin default".to_owned()
}

fn project_catalog_from_backend(projects: &[ProjectRecord]) -> ProjectCatalog {
    ProjectCatalog::new(
        projects
            .iter()
            .map(|project| ProjectInfo {
                canonical_name: project.canonical_name.clone(),
                aliases: project.aliases.clone(),
                path: PathBuf::from(format!("/registered/{}", project.canonical_name)),
            })
            .collect(),
    )
}

fn enrich_backend_error_for_cli(error: TrackError) -> TrackError {
    match error.code {
        ErrorCode::MigrationRequired => TrackError::new(
            ErrorCode::MigrationRequired,
            "Backend migration is required. Run `track migrate status` to inspect it, then `track migrate import` before using capture commands.",
        ),
        ErrorCode::ProjectNotFound => TrackError::new(
            ErrorCode::ProjectNotFound,
            "The selected project is not registered on the backend. Run `track project register` from a local checkout first.",
        ),
        ErrorCode::RemoteAgentNotConfigured => TrackError::new(
            ErrorCode::RemoteAgentNotConfigured,
            "Remote dispatch is not configured yet. Run `track remote-agent configure --host <host> --user <user> --identity-file ~/.ssh/track_remote_agent` first.",
        ),
        _ => error,
    }
}

fn resolve_register_path(path: Option<&Path>) -> Result<PathBuf, TrackError> {
    let path = match path {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => env::current_dir()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfigInput,
                    format!("Could not resolve the current directory: {error}"),
                )
            })?
            .join(path),
        None => env::current_dir().map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!("Could not resolve the current directory: {error}"),
            )
        })?,
    };

    Ok(path)
}

fn resolve_local_file(path: &Path, label: &str) -> Result<PathBuf, TrackError> {
    let resolved = resolve_register_path(Some(path))?;
    if !resolved.is_file() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "{label} must point to a readable file, but {} is not a file.",
                collapse_home_path(&resolved)
            ),
        ));
    }

    Ok(resolved)
}

fn read_optional_text_file(path: &Path, label: &str) -> Result<String, TrackError> {
    let resolved = resolve_local_file(path, label)?;
    fs::read_to_string(&resolved).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Could not read the {label} file at {}: {error}",
                collapse_home_path(&resolved)
            ),
        )
    })
}

fn canonicalize_aliases(
    aliases: Vec<String>,
    canonical_name: &str,
) -> Result<Vec<String>, TrackError> {
    let mut aliases = aliases
        .into_iter()
        .map(|alias| {
            validate_single_normal_path_component(
                &alias,
                "Project alias",
                ErrorCode::InvalidPathComponent,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    aliases.retain(|alias| alias != canonical_name);
    aliases.sort();
    aliases.dedup();
    Ok(aliases)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;

    use track_capture::{TaskParser, TaskParserFactory};
    use track_projects::project_catalog::ProjectCatalog;
    use track_projects::project_metadata::{ProjectMetadata, ProjectRecord};
    use track_types::errors::{ErrorCode, TrackError};
    use track_types::migration::{MigrationImportSummary, MigrationStatus};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        Confidence, ParsedTaskCandidate, Priority, Status, Task, TaskCreateInput, TaskSource,
    };

    use super::{
        canonicalize_aliases, format_created_task_output, run_capture_command_internal,
        run_project_register_command_internal, run_remote_agent_configure_command_internal,
        CliConfigService, LoadedCliConfig, ProjectRegisterArgs, RemoteAgentConfigureArgs,
    };
    use crate::backend_client::{
        ConfigureRemoteAgentRequest, RemoteAgentSettingsResponse, TrackBackend,
    };

    #[derive(Debug)]
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
            _config: &track_config::runtime::TrackRuntimeConfig,
        ) -> Result<Box<dyn TaskParser + 'static>, TrackError> {
            Ok(Box::new(StaticTaskParser {
                candidate: self.candidate.clone(),
            }))
        }
    }

    #[derive(Default)]
    struct FakeBackend {
        projects: Vec<ProjectRecord>,
        created_tasks: Mutex<Vec<TaskCreateInput>>,
        configured_remote_agents: Mutex<Vec<ConfigureRemoteAgentRequest>>,
        registered_projects: Mutex<Vec<(String, Vec<String>, ProjectMetadata)>>,
        fetch_projects_error: Option<TrackError>,
        create_task_error: Option<TrackError>,
    }

    impl TrackBackend for FakeBackend {
        fn fetch_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
            match self.fetch_projects_error.as_ref() {
                Some(error) => Err(TrackError::new(error.code, error.message())),
                None => Ok(self.projects.clone()),
            }
        }

        fn create_task(&self, input: &TaskCreateInput) -> Result<Task, TrackError> {
            if let Some(error) = self.create_task_error.as_ref() {
                return Err(TrackError::new(error.code, error.message()));
            }

            self.created_tasks
                .lock()
                .expect("created tasks mutex should not be poisoned")
                .push(input.clone());

            Ok(Task {
                id: "20260330-project-x-fix-a-bug".to_owned(),
                project: input.project.clone(),
                priority: input.priority,
                status: Status::Open,
                description: input.description.clone(),
                created_at: now_utc(),
                updated_at: now_utc(),
                source: input.source,
            })
        }

        fn migration_status(&self) -> Result<MigrationStatus, TrackError> {
            unimplemented!("migration_status is not used in these tests");
        }

        fn import_legacy_data(&self) -> Result<MigrationImportSummary, TrackError> {
            unimplemented!("import_legacy_data is not used in these tests");
        }

        fn configure_remote_agent(
            &self,
            input: &ConfigureRemoteAgentRequest,
        ) -> Result<RemoteAgentSettingsResponse, TrackError> {
            self.configured_remote_agents
                .lock()
                .expect("configured remote agents mutex should not be poisoned")
                .push(input.clone());

            Ok(RemoteAgentSettingsResponse {
                configured: true,
                host: Some(input.host.clone()),
                user: Some(input.user.clone()),
                port: Some(input.port),
            })
        }

        fn register_project(
            &self,
            canonical_name: &str,
            aliases: Vec<String>,
            metadata: ProjectMetadata,
        ) -> Result<ProjectRecord, TrackError> {
            self.registered_projects
                .lock()
                .expect("registered projects mutex should not be poisoned")
                .push((canonical_name.to_owned(), aliases.clone(), metadata.clone()));

            Ok(ProjectRecord {
                canonical_name: canonical_name.to_owned(),
                aliases,
                metadata,
            })
        }
    }

    fn loaded_cli_config(directory: &tempfile::TempDir) -> LoadedCliConfig {
        let service = CliConfigService::new(Some(directory.path().join("cli.json")), None)
            .expect("cli config service should resolve");
        service
            .load_or_initialize()
            .expect("cli config should initialize")
    }

    fn project_record(canonical_name: &str, aliases: Vec<&str>) -> ProjectRecord {
        ProjectRecord {
            canonical_name: canonical_name.to_owned(),
            aliases: aliases.into_iter().map(str::to_owned).collect(),
            metadata: ProjectMetadata {
                repo_url: format!("https://example.com/{canonical_name}"),
                git_url: format!("git@example.com:{canonical_name}.git"),
                base_branch: "main".to_owned(),
                description: None,
            },
        }
    }

    #[test]
    fn capture_uses_backend_project_catalog() {
        let directory = tempfile::TempDir::new().expect("tempdir should be created");
        let config_service = CliConfigService::new(Some(directory.path().join("cli.json")), None)
            .expect("cli config service should resolve");
        let loaded = loaded_cli_config(&directory);
        let backend = FakeBackend {
            projects: vec![project_record("project-x", vec!["proj-x"])],
            ..FakeBackend::default()
        };
        let parser_factory = StaticTaskParserFactory {
            candidate: ParsedTaskCandidate {
                project: Some("proj-x".to_owned()),
                priority: Priority::High,
                title: "Fix a bug".to_owned(),
                body_markdown: Some("- Inspect the parser output".to_owned()),
                confidence: Confidence::High,
                reason: None,
            },
        };

        let output = run_capture_command_internal(
            &[
                "proj-x".to_owned(),
                "fix".to_owned(),
                "a".to_owned(),
                "bug".to_owned(),
            ],
            &config_service,
            &loaded,
            &backend,
            &parser_factory,
        )
        .expect("capture command should succeed");

        assert!(output.contains("Created task"));
        let created_tasks = backend
            .created_tasks
            .lock()
            .expect("created tasks mutex should not be poisoned");
        assert_eq!(created_tasks.len(), 1);
        assert_eq!(created_tasks[0].project, "project-x");
        assert_eq!(created_tasks[0].source, Some(TaskSource::Cli));
    }

    #[test]
    fn capture_surfaces_migration_required_with_cli_guidance() {
        let directory = tempfile::TempDir::new().expect("tempdir should be created");
        let config_service = CliConfigService::new(Some(directory.path().join("cli.json")), None)
            .expect("cli config service should resolve");
        let loaded = loaded_cli_config(&directory);
        let backend = FakeBackend {
            fetch_projects_error: Some(TrackError::new(
                ErrorCode::MigrationRequired,
                "Backend requests are gated until migration is handled.",
            )),
            ..FakeBackend::default()
        };

        let error = run_capture_command_internal(
            &["project-x".to_owned(), "fix".to_owned()],
            &config_service,
            &loaded,
            &backend,
            &StaticTaskParserFactory {
                candidate: ParsedTaskCandidate {
                    project: Some("project-x".to_owned()),
                    priority: Priority::High,
                    title: "Fix a bug".to_owned(),
                    body_markdown: None,
                    confidence: Confidence::High,
                    reason: None,
                },
            },
        )
        .expect_err("capture should fail while migration is required");

        assert_eq!(error.code, ErrorCode::MigrationRequired);
        assert!(error.to_string().contains("track migrate status"));
    }

    #[test]
    fn project_register_sends_git_metadata_to_the_backend() {
        let directory = tempfile::TempDir::new().expect("tempdir should be created");
        let checkout = directory.path().join("project-x");
        fs::create_dir_all(checkout.join(".git")).expect("git directory should exist");
        fs::write(
            checkout.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:acme/project-x.git\n",
        )
        .expect("git config should be written");

        let config_service = CliConfigService::new(Some(directory.path().join("cli.json")), None)
            .expect("cli config service should resolve");
        let loaded = loaded_cli_config(&directory);
        let backend = FakeBackend::default();

        let output = run_project_register_command_internal(
            &config_service,
            &loaded,
            &backend,
            ProjectRegisterArgs {
                path: Some(checkout.clone()),
                aliases: vec!["proj-x".to_owned()],
            },
        )
        .expect("project registration should succeed");

        assert!(output.contains("Registered project"));
        let registered = backend
            .registered_projects
            .lock()
            .expect("registered projects mutex should not be poisoned");
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].0, "project-x");
        assert_eq!(registered[0].1, vec!["proj-x".to_owned()]);
        assert_eq!(
            registered[0].2.repo_url,
            "https://github.com/acme/project-x"
        );
        assert_eq!(registered[0].2.git_url, "git@github.com:acme/project-x.git");
    }

    #[test]
    fn remote_agent_configure_uploads_backend_owned_settings_and_secrets() {
        let directory = tempfile::TempDir::new().expect("tempdir should be created");
        let identity_file = directory.path().join("id_ed25519");
        let shell_prelude_file = directory.path().join("shell-prelude.sh");
        fs::write(
            &identity_file,
            "-----BEGIN OPENSSH PRIVATE KEY-----\nkey\n-----END OPENSSH PRIVATE KEY-----\n",
        )
        .expect("identity file should be written");
        fs::write(
            &shell_prelude_file,
            "export PATH=\"$HOME/.cargo/bin:$PATH\"\n",
        )
        .expect("shell prelude file should be written");

        let config_service = CliConfigService::new(Some(directory.path().join("cli.json")), None)
            .expect("cli config service should resolve");
        let loaded = loaded_cli_config(&directory);
        let backend = FakeBackend::default();

        let output = run_remote_agent_configure_command_internal(
            &config_service,
            &loaded,
            &backend,
            RemoteAgentConfigureArgs {
                host: "192.0.2.25".to_owned(),
                user: "builder".to_owned(),
                port: 2222,
                workspace_root: "~/workspace".to_owned(),
                projects_registry_path: "~/track-projects.json".to_owned(),
                identity_file: identity_file.clone(),
                known_hosts_file: None,
                shell_prelude: None,
                shell_prelude_file: Some(shell_prelude_file),
                enable_review_follow_up: false,
                main_user: Some("octocat".to_owned()),
                default_review_prompt: Some("Focus on regressions.".to_owned()),
                default_review_prompt_file: None,
            },
        )
        .expect("remote-agent configure should succeed");

        assert!(output.contains("Configured remote agent"));
        let configured = backend
            .configured_remote_agents
            .lock()
            .expect("configured remote agents mutex should not be poisoned");
        assert_eq!(configured.len(), 1);
        assert_eq!(configured[0].host, "192.0.2.25");
        assert_eq!(configured[0].user, "builder");
        assert_eq!(configured[0].port, 2222);
        assert_eq!(
            configured[0].shell_prelude.as_deref(),
            Some("export PATH=\"$HOME/.cargo/bin:$PATH\"\n")
        );
        assert_eq!(
            configured[0]
                .review_follow_up
                .as_ref()
                .and_then(|review_follow_up| review_follow_up.main_user.as_deref()),
            Some("octocat")
        );
    }

    #[test]
    fn drops_duplicate_project_aliases() {
        let aliases = canonicalize_aliases(
            vec![
                "proj-x".to_owned(),
                "proj-x".to_owned(),
                "project-x".to_owned(),
            ],
            "project-x",
        )
        .expect("aliases should validate");

        assert_eq!(aliases, vec!["proj-x".to_owned()]);
    }

    #[test]
    fn renders_created_task_output_without_a_file_path() {
        let rendered = format_created_task_output(&Task {
            id: "20260330-project-x-fix-a-bug".to_owned(),
            project: "project-x".to_owned(),
            priority: Priority::High,
            status: Status::Open,
            description: "Fix a bug".to_owned(),
            created_at: now_utc(),
            updated_at: now_utc(),
            source: Some(TaskSource::Cli),
        });

        assert!(rendered.contains("Created task"));
        assert!(rendered.contains("ID"));
    }
}
