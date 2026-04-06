use time::OffsetDateTime;

use crate::time_utils::format_task_id_timestamp;

define_path_id!(
    TaskId,
    "Task id",
    "database task ids should be valid path components"
);

impl TaskId {
    // TODO: If task id collisions need to be handled centrally again, restore a
    // shared suffixing strategy here instead of re-implementing it at call sites.
    pub fn unique(timestamp: OffsetDateTime, description: &str) -> Self {
        let base_id = format!(
            "{}-{}",
            format_task_id_timestamp(timestamp),
            build_task_slug(description)
        );

        Self::new(&base_id).expect("generated task ids should be valid path components")
    }
}

fn build_task_slug(description: &str) -> String {
    const TASK_SLUG_MAX_LENGTH: usize = 60;
    let slug = slug::slugify(description);
    let trimmed = slug.chars().take(TASK_SLUG_MAX_LENGTH).collect::<String>();

    if trimmed.is_empty() {
        "task".to_owned()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::{build_task_slug, TaskId};

    #[test]
    fn slug_falls_back_when_description_has_no_letters() {
        assert_eq!(build_task_slug("!!!"), "task");
    }

    #[test]
    fn unique_id_is_path_safe_by_construction() {
        let id = TaskId::unique(datetime!(2026-03-16 09:08:07 UTC), "Fix issue");

        assert_eq!(id.as_str(), "20260316-090807-fix-issue");
    }
}
