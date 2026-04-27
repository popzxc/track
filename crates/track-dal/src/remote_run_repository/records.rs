#[derive(Debug, sqlx::FromRow)]
pub(super) struct ActiveRemoteRunRow {
    pub(super) dispatch_id: String,
    pub(super) kind: String,
    pub(super) owner_id: Option<String>,
    pub(super) status: String,
}
