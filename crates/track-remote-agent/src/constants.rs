use time::Duration;

pub(crate) const REMOTE_PROMPT_FILE_NAME: &str = "prompt.md";
pub(crate) const REMOTE_SCHEMA_FILE_NAME: &str = "result-schema.json";

// Repository bootstrap can legitimately take a while on first clone or after a
// large fetch, so we keep the stale-preparing threshold generous. The API now
// also refreshes the summary at each preparation phase so normal progress keeps
// pushing this timeout forward instead of relying on one initial timestamp.
pub(crate) const PREPARING_STALE_AFTER: Duration = Duration::minutes(30);

pub(crate) const REVIEW_WORKTREE_DIRECTORY_NAME: &str = "review-worktrees";
