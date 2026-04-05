#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskDispatchRow {
    pub(super) dispatch_id: String,
    pub(super) task_id: String,
    pub(super) preferred_tool: String,
    pub(super) project: String,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) remote_host: String,
    pub(super) branch_name: Option<String>,
    pub(super) worktree_path: Option<String>,
    pub(super) pull_request_url: Option<String>,
    pub(super) follow_up_request: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) error_message: Option<String>,
    pub(super) review_request_head_oid: Option<String>,
    pub(super) review_request_user: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskIdRow {
    pub(super) task_id: String,
}
