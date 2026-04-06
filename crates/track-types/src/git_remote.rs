use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::errors::{ErrorCode, TrackError};

/// Git transport locator that can be handed to `git clone` or `git remote`.
///
/// Unlike `repo_url`, this value models Git's own transport syntax rather than
/// web URLs, so it intentionally accepts SCP-like SSH remotes, URI-style
/// remotes, and bare local filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitRemote(gix_url::Url);

impl GitRemote {
    pub fn new(value: &str) -> Result<Self, TrackError> {
        gix_url::Url::try_from(value).map(Self).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidGitRemote,
                format!("Git remote is not valid: {error}"),
            )
        })
    }

    pub fn from_db(value: String) -> Self {
        Self::new(&value).expect("database git remotes should stay valid")
    }

    pub fn github_ssh(owner: &str, repository: &str) -> Self {
        let remote = format!("git@github.com:{owner}/{repository}.git");
        Self::new(&remote)
            .expect("GitHub SSH remotes built from repository coordinates should parse")
    }

    /// Consumes this strong value at an application boundary and returns the
    /// serialized git remote string in its current transport form.
    pub fn into_remote_string(self) -> String {
        String::from_utf8(self.0.to_bstring().into())
            .expect("git remotes in application data should stay UTF-8")
    }
}

impl Serialize for GitRemote {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serialized = String::from_utf8(self.0.to_bstring().into())
            .expect("git remotes in application data should stay UTF-8");

        serializer.serialize_str(&serialized)
    }
}

impl<'de> Deserialize<'de> for GitRemote {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::errors::ErrorCode;

    use super::GitRemote;

    #[test]
    fn preserves_supported_git_remote_forms() {
        let vectors = [
            "git@github.com:acme/project-a.git",
            "ssh://git@example.com/project-a.git",
            "file:///tmp/project-a",
            "/srv/track-testing/git/upstream/project-a.git",
        ];

        for remote in vectors {
            let parsed = GitRemote::new(remote).expect("fixture git remotes should parse");

            assert_eq!(
                parsed.into_remote_string(),
                remote,
                "git remote should round-trip exactly for {remote}"
            );
        }
    }

    #[test]
    fn serde_uses_single_string_values() {
        let remote = GitRemote::new("git@github.com:acme/project-a.git").unwrap();

        let serialized = serde_json::to_value(&remote).expect("git remote should serialize");
        let roundtrip = serde_json::from_value::<GitRemote>(serialized.clone())
            .expect("git remote should deserialize");

        assert_eq!(serialized, json!("git@github.com:acme/project-a.git"));
        assert_eq!(
            roundtrip.into_remote_string(),
            "git@github.com:acme/project-a.git"
        );
    }

    #[test]
    fn rejects_empty_git_remotes() {
        let error = GitRemote::new("").expect_err("empty git remotes should fail");

        assert_eq!(error.code, ErrorCode::InvalidGitRemote);
    }

    #[test]
    fn github_ssh_builds_expected_remote() {
        let remote = GitRemote::github_ssh("acme", "project-a");

        assert_eq!(
            remote.into_remote_string(),
            "git@github.com:acme/project-a.git"
        );
    }

    #[test]
    fn deserializing_invalid_json_value_fails() {
        let error = serde_json::from_value::<GitRemote>(Value::Null)
            .expect_err("non-string git remotes should fail");

        assert!(error.to_string().contains("invalid type"));
    }
}
