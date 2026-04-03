use time::Duration;

pub(crate) const REMOTE_STATUS_FILE_NAME: &str = "status.txt";
pub(crate) const REMOTE_RESULT_FILE_NAME: &str = "result.json";
pub(crate) const REMOTE_STDERR_FILE_NAME: &str = "stderr.log";
pub(crate) const REMOTE_FINISHED_AT_FILE_NAME: &str = "finished-at.txt";
pub(crate) const REMOTE_PROMPT_FILE_NAME: &str = "prompt.md";
pub(crate) const REMOTE_SCHEMA_FILE_NAME: &str = "result-schema.json";
pub(crate) const REMOTE_LAUNCHER_PID_FILE_NAME: &str = "launcher.pid";

// We keep the historical sidecar filename for the child agent PID so users can
// still cancel or clean up runs that were launched before Claude support
// landed. The file now stores whichever remote agent process is active.
pub(crate) const REMOTE_CODEX_PID_FILE_NAME: &str = "codex.pid";

// Repository bootstrap can legitimately take a while on first clone or after a
// large fetch, so we keep the stale-preparing threshold generous. The API now
// also refreshes the summary at each preparation phase so normal progress keeps
// pushing this timeout forward instead of relying on one initial timestamp.
pub(crate) const PREPARING_STALE_AFTER: Duration = Duration::minutes(30);

pub(crate) const REVIEW_WORKTREE_DIRECTORY_NAME: &str = "review-worktrees";
pub(crate) const REVIEW_RUN_DIRECTORY_NAME: &str = "review-runs";
