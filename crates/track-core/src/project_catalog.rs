use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// =============================================================================
// Project Catalog
// =============================================================================
//
// Project discovery produces more than a flat list of repositories. The rest
// of the application needs one place that can answer:
// - which canonical projects exist
// - which aliases map onto them
// - which names should be exposed to the model prompt
//
// Keeping that logic in one domain type prevents alias handling from being
// split across discovery, prompt building, and capture validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    #[serde(rename = "canonicalName")]
    pub canonical_name: String,
    #[serde(with = "path_string")]
    pub path: PathBuf,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCatalog {
    projects: Vec<ProjectInfo>,
    lookup_by_name: BTreeMap<String, usize>,
}

impl ProjectCatalog {
    pub fn new(projects: Vec<ProjectInfo>) -> Self {
        let mut lookup_by_name = BTreeMap::new();

        for (index, project) in projects.iter().enumerate() {
            lookup_by_name
                .entry(normalize_lookup_key(&project.canonical_name))
                .or_insert(index);
        }

        for (index, project) in projects.iter().enumerate() {
            for alias in &project.aliases {
                lookup_by_name
                    .entry(normalize_lookup_key(alias))
                    .or_insert(index);
            }
        }

        Self {
            projects,
            lookup_by_name,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.projects.is_empty()
    }

    pub fn projects(&self) -> &[ProjectInfo] {
        &self.projects
    }

    pub fn into_projects(self) -> Vec<ProjectInfo> {
        self.projects
    }

    pub fn resolve(&self, name: &str) -> Option<&ProjectInfo> {
        let key = normalize_lookup_key(name);
        let index = self.lookup_by_name.get(&key)?;
        self.projects.get(*index)
    }
}

fn normalize_lookup_key(value: &str) -> String {
    value.trim().to_lowercase()
}

mod path_string {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(path: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&path.to_string_lossy())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = String::deserialize(deserializer)?;
        Ok(PathBuf::from(path))
    }
}
