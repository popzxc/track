use time::format_description::FormatItem;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime};

static ISO_8601_MILLIS_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
static ISO_8601_SECONDS_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

static TASK_ID_TIMESTAMP_FORMAT: &[FormatItem<'static>] =
    format_description!("[year][month][day]-[hour][minute][second]");

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub fn format_iso_8601_millis(value: OffsetDateTime) -> String {
    value
        .replace_millisecond(value.millisecond())
        .expect("millisecond replacement should stay in range")
        .format(ISO_8601_MILLIS_FORMAT)
        .expect("timestamp formatting should succeed")
}

pub fn parse_iso_8601_millis(value: &str) -> Result<OffsetDateTime, time::error::Parse> {
    PrimitiveDateTime::parse(value, ISO_8601_MILLIS_FORMAT).map(PrimitiveDateTime::assume_utc)
}

pub fn parse_iso_8601_seconds(value: &str) -> Result<OffsetDateTime, time::error::Parse> {
    PrimitiveDateTime::parse(value, ISO_8601_SECONDS_FORMAT).map(PrimitiveDateTime::assume_utc)
}

pub fn format_task_id_timestamp(value: OffsetDateTime) -> String {
    value
        .format(TASK_ID_TIMESTAMP_FORMAT)
        .expect("task id timestamp formatting should succeed")
}
