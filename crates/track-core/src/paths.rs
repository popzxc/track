use std::env;
use std::path::{Path, PathBuf};

use crate::errors::{ErrorCode, TrackError};

pub const DEFAULT_CONFIG_PATH: &str = "~/.config/track/config.json";
pub const DEFAULT_DATA_DIR: &str = "~/.track/issues";

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

pub fn expand_home_path(path_value: &str) -> PathBuf {
    match path_value {
        "~" => home_dir().unwrap_or_else(|| PathBuf::from("~")),
        value if value.starts_with("~/") => home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(&value[2..]),
        value => PathBuf::from(value),
    }
}

pub fn resolve_path_from_invocation_dir(path_value: &str) -> Result<PathBuf, TrackError> {
    let current_directory = env::current_dir().map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfig,
            format!("Could not resolve a configured path from the current directory: {error}"),
        )
    })?;

    Ok(resolve_path_from_base_dir(path_value, &current_directory))
}

pub fn resolve_path_from_config_file(
    path_value: &str,
    file_path: &Path,
) -> Result<PathBuf, TrackError> {
    let base_dir = file_path.parent().ok_or_else(|| {
        TrackError::new(
            ErrorCode::InvalidConfig,
            format!(
                "Could not resolve a configured path relative to config file {}.",
                collapse_home_path(file_path)
            ),
        )
    })?;

    Ok(resolve_path_from_base_dir(path_value, base_dir))
}

pub fn resolve_optional_command_path_from_config_file(
    path_value: Option<&str>,
    file_path: &Path,
) -> Result<Option<String>, TrackError> {
    let Some(path_value) = path_value else {
        return Ok(None);
    };

    if path_value.starts_with("~/")
        || path_value.starts_with("./")
        || path_value.starts_with("../")
        || path_value.contains('/')
    {
        return Ok(Some(path_to_string(&resolve_path_from_config_file(
            path_value, file_path,
        )?)));
    }

    Ok(Some(path_value.to_owned()))
}

pub fn get_config_path() -> Result<PathBuf, TrackError> {
    resolve_path_from_invocation_dir(
        &env::var("TRACK_CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_owned()),
    )
}

pub fn get_data_dir() -> Result<PathBuf, TrackError> {
    resolve_path_from_invocation_dir(
        &env::var("TRACK_DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.to_owned()),
    )
}

pub fn collapse_home_path(path: &Path) -> String {
    match home_dir() {
        Some(home) if path == home => "~".to_owned(),
        Some(home) if path.starts_with(&home) => {
            let relative = path.strip_prefix(home).unwrap_or(path);
            let relative = path_to_string(relative).trim_start_matches('/').to_owned();

            if relative.is_empty() {
                "~".to_owned()
            } else {
                format!("~/{relative}")
            }
        }
        _ => path_to_string(path),
    }
}

pub fn collapse_path_value(path_value: &str) -> String {
    collapse_home_path(&expand_home_path(path_value))
}

pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolve_path_from_base_dir(path_value: &str, base_dir: &Path) -> PathBuf {
    let expanded = expand_home_path(path_value);
    if expanded.is_absolute() {
        return expanded;
    }

    base_dir.join(expanded)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{collapse_home_path, collapse_path_value};

    #[test]
    fn collapses_home_relative_paths_with_a_slash() {
        let rendered = collapse_home_path(Path::new("/home/popzxc/.track/issues"));

        assert_eq!(rendered, "~/.track/issues");
    }

    #[test]
    fn collapses_home_prefixed_string_values() {
        assert_eq!(
            collapse_path_value("/home/popzxc/.config/track/config.json"),
            "~/.config/track/config.json"
        );
    }
}
