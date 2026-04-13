#[derive(Debug, sqlx::FromRow)]
pub(super) struct SettingJsonRecord {
    pub(super) setting_json: String,
}
