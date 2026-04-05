#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewRunRow {
    pub(super) dispatch_id: String,
    pub(super) review_id: String,
    pub(super) pull_request_url: String,
    pub(super) repository_full_name: String,
    pub(super) workspace_key: String,
    pub(super) preferred_tool: String,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) remote_host: String,
    pub(super) branch_name: Option<String>,
    pub(super) worktree_path: Option<String>,
    pub(super) follow_up_request: Option<String>,
    pub(super) target_head_oid: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) review_submitted: i64,
    pub(super) github_review_id: Option<String>,
    pub(super) github_review_url: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) error_message: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewIdRow {
    pub(super) review_id: String,
}
