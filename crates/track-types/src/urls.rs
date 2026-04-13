use crate::errors::{ErrorCode, TrackError};

pub use url::Url;

/// Parses an external URL value after trimming incidental surrounding
/// whitespace at the application boundary.
pub fn parse_url(
    value: &str,
    error_code: ErrorCode,
    invalid_message: impl Into<String>,
) -> Result<Url, TrackError> {
    Url::parse(value.trim()).map_err(|error| {
        TrackError::new(error_code, format!("{}: {error}", invalid_message.into()))
    })
}

/// Parses a persisted URL value that the application treats as a trusted
/// invariant once it has crossed into storage.
pub fn parse_persisted_url(value: String, expect_message: &'static str) -> Url {
    Url::parse(&value).expect(expect_message)
}

#[cfg(test)]
mod tests {
    use crate::errors::ErrorCode;

    use super::{parse_url, Url};

    #[test]
    fn parses_trimmed_external_urls() {
        let parsed = parse_url(
            " https://github.com/acme/project-a ",
            ErrorCode::InvalidProjectMetadata,
            "repo URL should parse",
        )
        .expect("trimmed url should parse");

        assert_eq!(
            parsed,
            Url::parse("https://github.com/acme/project-a").unwrap()
        );
    }
}
