use std::collections::BTreeMap;
use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::errors::{ErrorCode, TrackError};
use crate::project_catalog::{ProjectCatalog, ProjectInfo};
use crate::types::TrackRuntimeConfig;

const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    "target",
    ".next",
    ".turbo",
    ".venv",
];

fn is_ignored_directory(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .map(|name| IGNORED_DIRECTORIES.contains(&name))
            .unwrap_or(false)
}

fn has_git_marker(path: &Path) -> bool {
    path.join(".git").exists()
}

pub fn discover_projects(config: &TrackRuntimeConfig) -> Result<ProjectCatalog, TrackError> {
    let mut discovered_projects = BTreeMap::<String, ProjectInfo>::new();

    // We discover canonical project names from the filesystem first and only
    // then layer aliases on top. That keeps aliases from inventing projects.
    for root in &config.project_roots {
        if !root.exists() {
            continue;
        }

        // Once we have identified a repository root, deeper files inside that
        // repository cannot produce a second canonical project name. We stop
        // descending there so scan cost tracks repository count rather than
        // repository size.
        //
        // TODO: If nested repositories become an intentional first-class use
        // case, revisit this pruning strategy and add an explicit opt-in for
        // them instead of walking every working tree recursively by default.
        let mut walker = WalkDir::new(root).follow_links(false).into_iter();

        while let Some(entry) = walker.next() {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!("Could not scan configured project roots: {error}"),
                )
            })?;

            if is_ignored_directory(&entry) {
                walker.skip_current_dir();
                continue;
            }

            if !entry.file_type().is_dir() {
                continue;
            }

            if !has_git_marker(entry.path()) {
                continue;
            }

            walker.skip_current_dir();

            let canonical_name = entry
                .path()
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();

            if canonical_name.is_empty() {
                continue;
            }

            let key = canonical_name.to_lowercase();
            discovered_projects
                .entry(key)
                .or_insert_with(|| ProjectInfo {
                    canonical_name,
                    path: entry.path().to_path_buf(),
                    aliases: Vec::new(),
                });
        }
    }

    for (alias, canonical_name) in &config.project_aliases {
        if let Some(project) = discovered_projects.get_mut(&canonical_name.to_lowercase()) {
            project.aliases.push(alias.clone());
        }
    }

    let mut projects = discovered_projects.into_values().collect::<Vec<_>>();
    projects.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
    Ok(ProjectCatalog::new(projects))
}
