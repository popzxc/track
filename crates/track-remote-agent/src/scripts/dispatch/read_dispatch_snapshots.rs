use serde::Serialize;
use track_types::errors::{ErrorCode, TrackError};

use crate::constants::{
    REMOTE_FINISHED_AT_FILE_NAME, REMOTE_RESULT_FILE_NAME, REMOTE_STATUS_FILE_NAME,
    REMOTE_STDERR_FILE_NAME,
};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;
use crate::types::RemoteDispatchSnapshot;

const READ_DISPATCH_SNAPSHOTS_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/read_dispatch_snapshots.sh.tera");

/// Reads the status files for one or more remote run directories and decodes
/// them into structured snapshots.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ReadDispatchSnapshotsScript;

impl ReadDispatchSnapshotsScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            READ_DISPATCH_SNAPSHOTS_TEMPLATE,
            &ReadDispatchSnapshotsTemplate {
                path_helpers: remote_path_helpers_shell(),
                status_file: REMOTE_STATUS_FILE_NAME,
                result_file: REMOTE_RESULT_FILE_NAME,
                stderr_file: REMOTE_STDERR_FILE_NAME,
                finished_at_file: REMOTE_FINISHED_AT_FILE_NAME,
            },
        )
    }

    pub(crate) fn parse_report(
        &self,
        report: &str,
    ) -> Result<Vec<RemoteDispatchSnapshot>, TrackError> {
        let mut snapshots = Vec::new();
        let mut current_snapshot: Option<RemoteDispatchSnapshot> = None;

        for line in report.lines().filter(|line| !line.trim().is_empty()) {
            let columns = line.splitn(3, '\t').collect::<Vec<_>>();
            match columns.first().copied() {
                Some("run") => {
                    let _run_identifier = columns.get(1).ok_or_else(|| {
                        TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            "Remote dispatch refresh report is missing a run directory.",
                        )
                    })?;
                    if let Some(snapshot) = current_snapshot.take() {
                        snapshots.push(snapshot);
                    }
                    current_snapshot = Some(RemoteDispatchSnapshot::default());
                }
                Some("status") | Some("result") | Some("stderr") | Some("finished_at") => {
                    let field_name = columns
                        .first()
                        .expect("field-tagged dispatch line should have a tag");
                    let presence = columns.get(1).ok_or_else(|| {
                        TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            "Remote dispatch refresh report is missing a field state.",
                        )
                    })?;
                    let value = match *presence {
                        "missing" => None,
                        "present" => {
                            Some(decode_hex_string(columns.get(2).copied().unwrap_or(""))?)
                        }
                        _ => {
                            return Err(TrackError::new(
                                ErrorCode::RemoteDispatchFailed,
                                "Remote dispatch refresh report has an unknown field state.",
                            ));
                        }
                    };
                    let Some(snapshot) = current_snapshot.as_mut() else {
                        return Err(TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            "Remote dispatch refresh report emitted a field before the run header.",
                        ));
                    };
                    match *field_name {
                        "status" => snapshot.set_status_from_file_contents(value),
                        "result" => snapshot.set_result(value),
                        "stderr" => snapshot.set_stderr(value),
                        "finished_at" => snapshot.set_finished_at(value),
                        _ => {}
                    }
                }
                _ => {
                    return Err(TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        "Remote dispatch refresh report contains an unexpected line.",
                    ));
                }
            }
        }

        if let Some(snapshot) = current_snapshot {
            snapshots.push(snapshot);
        }

        Ok(snapshots)
    }
}

#[derive(Serialize)]
struct ReadDispatchSnapshotsTemplate<'a> {
    path_helpers: &'a str,
    status_file: &'a str,
    result_file: &'a str,
    stderr_file: &'a str,
    finished_at_file: &'a str,
}

fn decode_hex_string(hex: &str) -> Result<String, TrackError> {
    if !hex.len().is_multiple_of(2) {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            "Remote dispatch refresh data is not valid hexadecimal.",
        ));
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut index = 0;
    while index < hex.len() {
        let byte = u8::from_str_radix(&hex[index..index + 2], 16).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Remote dispatch refresh data is not valid hexadecimal: {error}"),
            )
        })?;
        bytes.push(byte);
        index += 2;
    }

    String::from_utf8(bytes).map_err(|error| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Remote dispatch refresh data is not valid UTF-8: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::types::RemoteRunStatus;

    use super::ReadDispatchSnapshotsScript;

    #[test]
    fn parses_batched_dispatch_snapshot_report() {
        let report = concat!(
            "run\t~/workspace/project-x/dispatches/dispatch-1\n",
            "status\tpresent\t72756e6e696e670a\n",
            "result\tmissing\t\n",
            "stderr\tmissing\t\n",
            "finished_at\tmissing\t\n",
            "run\t~/workspace/project-y/dispatches/dispatch-2\n",
            "status\tpresent\t636f6d706c657465640a\n",
            "result\tpresent\t7b22737461747573223a22737563636565646564227d\n",
            "stderr\tpresent\t\n",
            "finished_at\tpresent\t323032362d30332d31385431303a33353a33315a0a\n",
        );

        let snapshots = ReadDispatchSnapshotsScript
            .parse_report(report)
            .expect("dispatch snapshot report should parse");

        assert_eq!(
            snapshots
                .first()
                .expect("first dispatch snapshot should exist")
                .status(),
            &RemoteRunStatus::Running
        );
        assert_eq!(
            snapshots
                .get(1)
                .expect("second dispatch snapshot should exist")
                .required_result("completed snapshot should keep the parsed result")
                .ok(),
            Some("{\"status\":\"succeeded\"}")
        );
        assert_eq!(
            snapshots
                .get(1)
                .expect("second dispatch snapshot should exist")
                .finished_at_or(time::OffsetDateTime::UNIX_EPOCH),
            time::macros::datetime!(2026-03-18 10:35:31 UTC)
        );
    }

    #[test]
    fn preserves_unexpected_remote_status_values() {
        let report = concat!(
            "run\t~/workspace/project-x/dispatches/dispatch-1\n",
            "status\tpresent\t7761740a\n",
            "result\tmissing\t\n",
            "stderr\tmissing\t\n",
            "finished_at\tmissing\t\n",
        );

        let snapshots = ReadDispatchSnapshotsScript
            .parse_report(report)
            .expect("dispatch snapshot report should parse");

        assert_eq!(
            snapshots
                .first()
                .expect("dispatch snapshot should exist")
                .status(),
            &RemoteRunStatus::Incorrect("wat".to_owned())
        );
    }
}
