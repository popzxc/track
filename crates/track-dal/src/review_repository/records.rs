#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewRow {
    pub(super) id: String,
    pub(super) pull_request_url: String,
    pub(super) pull_request_number: i64,
    pub(super) pull_request_title: String,
    pub(super) repository_full_name: String,
    pub(super) repo_url: String,
    pub(super) git_url: String,
    pub(super) base_branch: String,
    pub(super) workspace_key: String,
    pub(super) preferred_tool: String,
    pub(super) project: Option<String>,
    pub(super) main_user: String,
    pub(super) default_review_prompt: Option<String>,
    pub(super) extra_instructions: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}
