use crate::time_utils::format_task_id_timestamp;

pub const TASK_SLUG_MAX_LENGTH: usize = 60;

pub fn build_task_slug(description: &str) -> String {
    let slug = slug::slugify(description);
    let trimmed = slug.chars().take(TASK_SLUG_MAX_LENGTH).collect::<String>();

    if trimmed.is_empty() {
        "task".to_owned()
    } else {
        trimmed
    }
}

// TODO: What the hell is this function (or rather 'was')
pub fn build_unique_task_id(
    timestamp: time::OffsetDateTime,
    description: &str,
    // mut exists: F,
) -> String {
    let base_id = format!(
        "{}-{}",
        format_task_id_timestamp(timestamp),
        build_task_slug(description)
    );

    base_id

    // if !exists(&base_id) {
    //     return base_id;
    // }

    // let mut suffix = 2;
    // loop {
    //     let candidate = format!("{base_id}-{suffix}");
    //     if !exists(&candidate) {
    //         return candidate;
    //     }

    //     suffix += 1;
    // }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::{build_task_slug, build_unique_task_id};

    #[test]
    fn slug_falls_back_when_description_has_no_letters() {
        assert_eq!(build_task_slug("!!!"), "task");
    }

    // #[test]
    // fn unique_id_appends_suffix_when_needed() {
    //     let id = build_unique_task_id(
    //         datetime!(2026-03-16 09:08:07 UTC),
    //         "Fix issue",
    //         |candidate| candidate == "20260316-090807-fix-issue",
    //     );

    //     assert_eq!(id, "20260316-090807-fix-issue-2");
    // }
}
