use std::path::{Component, Path};

use crate::errors::{ErrorCode, TrackError};

// =============================================================================
// Storage Path Component Validation
// =============================================================================
//
// Several domain identifiers eventually become directories or filenames under
// the track data root. Validating them in one shared helper keeps every caller
// on the same safety contract instead of relying on each HTTP or CLI entrypoint
// to remember its own path traversal checks.
pub fn validate_single_normal_path_component(
    value: &str,
    field_name: &str,
    error_code: ErrorCode,
) -> Result<String, TrackError> {
    let trimmed = value.trim();

    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
        return Err(invalid_path_component(field_name, error_code));
    }

    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(trimmed.to_owned()),
        _ => Err(invalid_path_component(field_name, error_code)),
    }
}

fn invalid_path_component(field_name: &str, error_code: ErrorCode) -> TrackError {
    TrackError::new(
        error_code,
        format!(
            "{field_name} must be one non-empty path component without separators or `.` / `..`."
        ),
    )
}

#[cfg(test)]
mod tests {
    use crate::errors::ErrorCode;

    use super::validate_single_normal_path_component;

    #[test]
    fn accepts_a_single_normal_component() {
        let validated = validate_single_normal_path_component(
            " project-x ",
            "Task project",
            ErrorCode::InvalidPathComponent,
        )
        .expect("single normal components should validate");

        assert_eq!(validated, "project-x");
    }

    #[test]
    fn rejects_values_that_are_not_single_normal_components() {
        for invalid in ["", ".", "..", "project/x", "project\\x", "project-x/"] {
            let error = validate_single_normal_path_component(
                invalid,
                "Task project",
                ErrorCode::InvalidPathComponent,
            )
            .expect_err("invalid path components should be rejected");

            assert_eq!(error.code, ErrorCode::InvalidPathComponent);
        }
    }
}
