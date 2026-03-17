use crate::config::ConfigService;
use crate::errors::{ErrorCode, TrackError};
use crate::llama_cpp::LlamaCppTaskParser;
use crate::project_catalog::ProjectCatalog;
use crate::project_discovery::discover_projects;
use crate::task_repository::FileTaskRepository;
use crate::types::{Confidence, ParsedTaskCandidate, StoredTask, TaskCreateInput, TaskSource};

fn validate_parsed_task_candidate(
    candidate: ParsedTaskCandidate,
    project_catalog: &ProjectCatalog,
) -> Result<(String, crate::types::Priority, String), TrackError> {
    if candidate.project.is_none() || candidate.confidence == Confidence::Low {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectSelection,
            "Could not determine a valid project from your input. Please mention one of the allowed project names or aliases more explicitly.",
        ));
    }

    let Some(project) = project_catalog.resolve(&candidate.project.unwrap_or_default()) else {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectSelection,
            "Could not determine a valid project from your input. Please mention one of the allowed project names or aliases more explicitly.",
        ));
    };

    Ok((
        project.canonical_name.clone(),
        candidate.priority,
        candidate.description.trim().to_owned(),
    ))
}

pub struct TaskCaptureService<'a> {
    pub config_service: &'a ConfigService,
    pub task_repository: &'a FileTaskRepository,
}

impl<'a> TaskCaptureService<'a> {
    pub fn create_task_from_text(
        &self,
        raw_text: &str,
        source: Option<TaskSource>,
    ) -> Result<StoredTask, TrackError> {
        if raw_text.trim().is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a task description.",
            ));
        }

        let config = self.config_service.load_runtime_config()?;
        if config.project_roots.is_empty() {
            return Err(TrackError::new(
                ErrorCode::NoProjectRoots,
                "No project roots configured.",
            ));
        }

        let project_catalog = discover_projects(&config)?;
        if project_catalog.is_empty() {
            return Err(TrackError::new(
                ErrorCode::NoProjectsDiscovered,
                "No git repositories found under configured roots.",
            ));
        }

        let parser = LlamaCppTaskParser::new(
            config.llama_cpp.llama_completion_path.clone(),
            config.llama_cpp.model_path.clone(),
        );
        let candidate = parser.parse_task(raw_text, &project_catalog)?;
        let (project, priority, description) =
            validate_parsed_task_candidate(candidate, &project_catalog)?;

        self.task_repository.create_task(TaskCreateInput {
            project,
            priority,
            description,
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::validate_parsed_task_candidate;
    use crate::project_catalog::{ProjectCatalog, ProjectInfo};
    use crate::types::{Confidence, ParsedTaskCandidate, Priority};

    #[test]
    fn resolves_aliases_to_canonical_project_names() {
        let project_catalog = ProjectCatalog::new(vec![ProjectInfo {
            canonical_name: "project-x".to_owned(),
            path: PathBuf::from("/tmp/project-x"),
            aliases: vec!["proj-x".to_owned()],
        }]);

        let (canonical_name, priority, description) = validate_parsed_task_candidate(
            ParsedTaskCandidate {
                project: Some("proj-x".to_owned()),
                priority: Priority::High,
                description: "Ship the alias path".to_owned(),
                confidence: Confidence::High,
                reason: None,
            },
            &project_catalog,
        )
        .expect("aliases should resolve to their canonical project");

        assert_eq!(canonical_name, "project-x");
        assert_eq!(priority, Priority::High);
        assert_eq!(description, "Ship the alias path");
    }
}
