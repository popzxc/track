use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use track_config::runtime::TrackRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::ProjectId;

use crate::project_catalog::{ProjectCatalog, ProjectInfo};

fn has_git_marker(path: &Path) -> bool {
    path.join(".git").exists()
}

pub fn discover_projects(config: &TrackRuntimeConfig) -> Result<ProjectCatalog, TrackError> {
    discover_projects_from_roots(&config.project_roots, &config.project_aliases)
}

pub fn discover_projects_from_roots(
    project_roots: &[PathBuf],
    project_aliases: &BTreeMap<String, String>,
) -> Result<ProjectCatalog, TrackError> {
    let mut discovered_projects = BTreeMap::<String, ProjectInfo>::new();

    // Project roots are directories that directly contain repositories. We do
    // not recurse because recursive discovery makes project registration
    // implicit and surprising:
    // - nested repositories become visible without being named as roots
    // - git submodules get misclassified as first-class projects
    // - ignore lists become an unbounded policy surface
    //
    // Requiring each nesting level to be an explicit configured root keeps the
    // discovery contract predictable and cheap to evaluate.
    for root in project_roots {
        if !root.exists() {
            continue;
        }

        let entries = fs::read_dir(root).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not scan configured project roots: {error}"),
            )
        })?;

        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!("Could not scan configured project roots: {error}"),
                )
            })?;

            let file_type = entry.file_type().map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!("Could not scan configured project roots: {error}"),
                )
            })?;
            if !file_type.is_dir() {
                continue;
            }

            let path = entry.path();
            if !has_git_marker(&path) {
                continue;
            }

            let canonical_name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();

            let Ok(canonical_name) = ProjectId::new(&canonical_name) else {
                continue;
            };

            let key = canonical_name.as_str().to_lowercase();
            discovered_projects
                .entry(key)
                .or_insert_with(|| ProjectInfo {
                    canonical_name,
                    path,
                    aliases: Vec::new(),
                });
        }
    }

    for (alias, canonical_name) in project_aliases {
        if let Some(project) = discovered_projects.get_mut(&canonical_name.to_lowercase()) {
            project.aliases.push(ProjectId::new(alias)?);
        }
    }

    let mut projects = discovered_projects.into_values().collect::<Vec<_>>();
    projects.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    Ok(ProjectCatalog::new(projects))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::discover_projects_from_roots;

    fn create_git_project(path: &Path) {
        fs::create_dir_all(path.join(".git")).expect("fixture git directory should be created");
    }

    #[test]
    fn discovers_immediate_child_repositories_from_each_root() {
        let directory = TempDir::new().expect("tempdir should be created");
        let workspace_root = directory.path().join("workspace");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");
        create_git_project(&workspace_root.join("project-a"));
        create_git_project(&workspace_root.join("project-b"));

        let catalog =
            discover_projects_from_roots(&[workspace_root], &BTreeMap::new()).expect("should scan");

        let project_names = catalog
            .projects()
            .iter()
            .map(|project| project.canonical_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(project_names, vec!["project-a", "project-b"]);
    }

    #[test]
    fn does_not_recurse_into_nested_directories_or_submodules() {
        let directory = TempDir::new().expect("tempdir should be created");
        let workspace_root = directory.path().join("workspace");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");

        let parent = workspace_root.join("container");
        fs::create_dir_all(&parent).expect("parent directory should exist");
        create_git_project(&parent.join("nested-project"));
        create_git_project(&workspace_root.join("actual-project"));

        let catalog =
            discover_projects_from_roots(&[workspace_root], &BTreeMap::new()).expect("should scan");

        let project_names = catalog
            .projects()
            .iter()
            .map(|project| project.canonical_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(project_names, vec!["actual-project"]);
    }

    #[test]
    fn does_not_treat_the_root_directory_itself_as_a_project() {
        let directory = TempDir::new().expect("tempdir should be created");
        create_git_project(directory.path());

        let catalog =
            discover_projects_from_roots(&[directory.path().to_path_buf()], &BTreeMap::new())
                .expect("should scan");

        assert!(catalog.projects().is_empty());
    }

    #[test]
    fn attaches_aliases_only_to_directly_discovered_projects() {
        let directory = TempDir::new().expect("tempdir should be created");
        let workspace_root = directory.path().join("workspace");
        fs::create_dir_all(&workspace_root).expect("workspace root should exist");
        create_git_project(&workspace_root.join("project-a"));

        let aliases = BTreeMap::from([(String::from("proj-a"), String::from("project-a"))]);
        let catalog =
            discover_projects_from_roots(&[workspace_root], &aliases).expect("should scan");

        let project = catalog
            .projects()
            .first()
            .expect("one project should be discovered");
        assert_eq!(project.canonical_name, "project-a");
        assert_eq!(
            project
                .aliases
                .iter()
                .map(|alias| alias.as_str())
                .collect::<Vec<_>>(),
            vec!["proj-a"],
        );
    }
}
