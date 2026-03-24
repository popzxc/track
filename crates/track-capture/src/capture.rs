use track_core::config::ConfigService;
use track_core::errors::{ErrorCode, TrackError};
use track_core::project_catalog::{ProjectCatalog, ProjectInfo};
use track_core::project_discovery::discover_projects;
use track_core::project_repository::ProjectRepository;
use track_core::task_description::render_task_description;
use track_core::task_repository::FileTaskRepository;
use track_core::types::{
    Confidence, ParsedTaskCandidate, Priority, StoredTask, TaskCreateInput, TaskSource,
};

use crate::task_parser::TaskParserFactory;

fn validate_parsed_task_candidate(
    candidate: ParsedTaskCandidate,
    project_catalog: &ProjectCatalog,
) -> Result<(ProjectInfo, Priority, String, String), TrackError> {
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

    let title = candidate.title.trim().to_owned();
    if title.is_empty() {
        return Err(TrackError::new(
            ErrorCode::AiParseFailed,
            "AI parse failure. The local model did not return a task title.",
        ));
    }

    Ok((
        project.clone(),
        candidate.priority,
        title,
        candidate
            .body_markdown
            .unwrap_or_default()
            .trim()
            .to_owned(),
    ))
}

// =============================================================================
// CLI Capture Body Assembly
// =============================================================================
//
// The model is good at extracting routing fields and shaping a readable task
// body, but it still should not be the only holder of user intent. We want the
// pleasant Markdown it can produce, while still preserving the exact original
// note whenever it contains context the formatted body might smooth over.
//
// The saved Markdown therefore has three layers:
// 1. a concise title line
// 2. optional model-authored supporting Markdown
// 3. the original note appended at the bottom when it adds information
fn build_task_body(
    title: &str,
    body_markdown: &str,
    raw_text: &str,
    project_catalog: &ProjectCatalog,
    canonical_project: &str,
    priority: Priority,
) -> String {
    let normalized_title = title.trim();
    let formatted_body = sanitize_model_body(normalized_title, body_markdown);
    let original_note =
        strip_capture_shorthand(raw_text, project_catalog, canonical_project, priority);
    let rendered_without_original = render_task_description(
        normalized_title,
        (!formatted_body.is_empty()).then_some(formatted_body.as_str()),
        None,
    );
    let preserved_original_note = if !original_note.is_empty()
        && normalize_for_comparison(&original_note)
            != normalize_for_comparison(&rendered_without_original)
    {
        Some(original_note.as_str())
    } else {
        None
    };

    render_task_description(
        normalized_title,
        (!formatted_body.is_empty()).then_some(formatted_body.as_str()),
        preserved_original_note,
    )
}

fn sanitize_model_body(title: &str, body_markdown: &str) -> String {
    let mut lines = body_markdown.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    // Local models sometimes repeat the title as the first line or as a
    // Markdown heading even when we ask them not to. We trim that duplication
    // so the saved task reads like intentional prose instead of prompt drift.
    loop {
        let Some(first_non_empty_index) = lines.iter().position(|line| !line.trim().is_empty())
        else {
            return String::new();
        };

        let leading_line = strip_markdown_heading(lines[first_non_empty_index]);
        if normalize_for_comparison(leading_line) != normalize_for_comparison(title) {
            break;
        }

        lines.drain(..=first_non_empty_index);
    }

    lines.join("\n").trim().to_owned()
}

fn strip_capture_shorthand(
    raw_text: &str,
    project_catalog: &ProjectCatalog,
    canonical_project: &str,
    priority: Priority,
) -> String {
    let mut remainder = raw_text.trim();
    let mut stripped_prefix = false;

    // The common CLI shape is `track <project> prio <priority> ...`. When we
    // echo the original note back into the saved task, we remove that routing
    // shorthand so the persisted context reads like a task note, not like a
    // shell command transcript.
    if let Some((candidate_project, rest)) = split_first_token(remainder) {
        let matches_project = project_catalog
            .resolve(candidate_project)
            .map(|project| project.canonical_name == canonical_project)
            .unwrap_or(false);

        if matches_project {
            remainder = rest;
            stripped_prefix = true;
        }
    }

    if let Some((keyword, rest)) = split_first_token(remainder) {
        let is_priority_keyword =
            keyword.eq_ignore_ascii_case("prio") || keyword.eq_ignore_ascii_case("priority");

        if is_priority_keyword {
            if let Some((value, tail)) = split_first_token(rest) {
                if value.eq_ignore_ascii_case(priority.as_str()) {
                    remainder = tail;
                    stripped_prefix = true;
                }
            }
        }
    }

    let trimmed = remainder.trim();
    if !stripped_prefix {
        return trimmed.to_owned();
    }

    trimmed
        .trim_start_matches(|character: char| {
            character.is_whitespace() || character == ':' || character == '-'
        })
        .trim()
        .to_owned()
}

fn split_first_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.find(char::is_whitespace) {
        Some(index) => Some((&trimmed[..index], &trimmed[index..])),
        None => Some((trimmed, "")),
    }
}

fn normalize_for_comparison(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn strip_markdown_heading(line: &str) -> &str {
    line.trim()
        .trim_start_matches('#')
        .trim_start()
        .trim_end_matches(':')
        .trim()
}

pub struct TaskCaptureService<'a> {
    pub config_service: &'a ConfigService,
    pub project_repository: &'a ProjectRepository,
    pub task_repository: &'a FileTaskRepository,
    pub task_parser_factory: &'a dyn TaskParserFactory,
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

        let parser = self.task_parser_factory.create_parser(&config)?;
        let candidate = parser.parse_task(raw_text, &project_catalog)?;
        let (project, priority, title, body_markdown) =
            validate_parsed_task_candidate(candidate, &project_catalog)?;
        let description = build_task_body(
            &title,
            &body_markdown,
            raw_text,
            &project_catalog,
            &project.canonical_name,
            priority,
        );

        // The CLI is the only component that can reliably inspect host
        // repositories, so task capture seeds `PROJECT.md` before the task hits
        // disk. The API can then work entirely from the persisted `.track`
        // directory, including inside Docker where source checkouts are not
        // mounted.
        self.project_repository.ensure_project(&project)?;

        self.task_repository.create_task(TaskCreateInput {
            project: project.canonical_name,
            priority,
            description,
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use track_core::project_catalog::{ProjectCatalog, ProjectInfo};
    use track_core::types::{Confidence, ParsedTaskCandidate, Priority};

    use super::{build_task_body, validate_parsed_task_candidate};

    #[test]
    fn resolves_aliases_to_canonical_project_names() {
        let project_catalog = ProjectCatalog::new(vec![ProjectInfo {
            canonical_name: "project-x".to_owned(),
            path: PathBuf::from("/tmp/project-x"),
            aliases: vec!["proj-x".to_owned()],
        }]);

        let (project, priority, title, body_markdown) = validate_parsed_task_candidate(
            ParsedTaskCandidate {
                project: Some("proj-x".to_owned()),
                priority: Priority::High,
                title: "Ship the alias path".to_owned(),
                body_markdown: Some(String::new()),
                confidence: Confidence::High,
                reason: None,
            },
            &project_catalog,
        )
        .expect("aliases should resolve to their canonical project");

        assert_eq!(project.canonical_name, "project-x");
        assert_eq!(priority, Priority::High);
        assert_eq!(title, "Ship the alias path");
        assert_eq!(body_markdown, "");
    }

    #[test]
    fn preserves_raw_capture_context_when_it_contains_more_than_the_summary() {
        let project_catalog = ProjectCatalog::new(vec![ProjectInfo {
            canonical_name: "zksync-airbender".to_owned(),
            path: PathBuf::from("/tmp/zksync-airbender"),
            aliases: vec!["airbender".to_owned()],
        }]);

        let body = build_task_body(
            "Off-by-one error in cycle markers",
            "- File: `riscv_transpiler/src/cycle/markers.rs`\n- Context: https://github.com/matter-labs/zksync-airbender/pull/237#discussion_r2950033943",
            "airbender prio high off-by-one error in riscv_transpiler/src/cycle/markers.rs -- https://github.com/matter-labs/zksync-airbender/pull/237#discussion_r2950033943 for context",
            &project_catalog,
            "zksync-airbender",
            Priority::High,
        );

        assert_eq!(
            body,
            "Off-by-one error in cycle markers\n\n## Summary\n\n- File: `riscv_transpiler/src/cycle/markers.rs`\n- Context: https://github.com/matter-labs/zksync-airbender/pull/237#discussion_r2950033943\n\n## Original note\n\n> off-by-one error in riscv_transpiler/src/cycle/markers.rs -- https://github.com/matter-labs/zksync-airbender/pull/237#discussion_r2950033943 for context"
        );
    }

    #[test]
    fn avoids_duplicating_context_when_the_note_matches_the_summary() {
        let project_catalog = ProjectCatalog::new(vec![ProjectInfo {
            canonical_name: "project-x".to_owned(),
            path: PathBuf::from("/tmp/project-x"),
            aliases: vec!["proj-x".to_owned()],
        }]);

        let body = build_task_body(
            "Fix a bug in module A",
            "",
            "proj-x prio high fix a bug in module A",
            &project_catalog,
            "project-x",
            Priority::High,
        );

        assert_eq!(body, "Fix a bug in module A");
    }

    #[test]
    fn removes_repeated_title_from_the_formatted_markdown_body() {
        let project_catalog = ProjectCatalog::new(vec![ProjectInfo {
            canonical_name: "project-x".to_owned(),
            path: PathBuf::from("/tmp/project-x"),
            aliases: vec![],
        }]);

        let body = build_task_body(
            "Investigate flaky integration test",
            "# Investigate flaky integration test\n\n- Repro: `cargo test --workspace`\n- Note: only fails on CI",
            "project-x investigate flaky integration test",
            &project_catalog,
            "project-x",
            Priority::Medium,
        );

        assert_eq!(
            body,
            "Investigate flaky integration test\n\n## Summary\n\n- Repro: `cargo test --workspace`\n- Note: only fails on CI\n\n## Original note\n\n> investigate flaky integration test"
        );
    }
}
