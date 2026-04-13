#[derive(Debug, sqlx::FromRow)]
pub(super) struct ProjectRow {
    pub(super) canonical_name: String,
    pub(super) repo_url: String,
    pub(super) git_url: String,
    pub(super) base_branch: String,
    pub(super) description: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ProjectAliasRow {
    pub(super) alias: String,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ProjectAliasListingRow {
    pub(super) canonical_name: String,
    pub(super) alias: String,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct AliasOwnerRow {
    pub(super) canonical_name: String,
}
