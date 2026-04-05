#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskRow {
    pub(super) id: String,
    pub(super) project: String,
    pub(super) priority: String,
    pub(super) status: String,
    pub(super) description: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) source: Option<String>,
}
