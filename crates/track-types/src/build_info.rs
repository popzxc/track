use serde::{Deserialize, Serialize};

// =============================================================================
// Shared Build Identity
// =============================================================================
//
// The CLI and the local server are released from the same workspace and are
// expected to move in lockstep. We keep their build metadata in one shared data
// model so the API can serialize it directly and the CLI can compare the two
// sides without inventing an ad hoc JSON shape in each crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub component: String,
    pub version: String,
    #[serde(rename = "gitCommit")]
    pub git_commit: String,
}

impl BuildInfo {
    pub fn new(
        component: impl Into<String>,
        version: impl Into<String>,
        git_commit: impl Into<String>,
    ) -> Self {
        Self {
            component: component.into(),
            version: version.into(),
            git_commit: git_commit.into(),
        }
    }

    pub fn matches_release(&self, other: &Self) -> bool {
        self.version == other.version && self.git_commit == other.git_commit
    }

    pub fn release_label(&self) -> String {
        format!("{} ({})", self.version, self.git_commit)
    }
}
