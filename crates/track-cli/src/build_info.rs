use track_types::build_info::BuildInfo;

pub const CLI_VERSION_TEXT: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TRACK_GIT_COMMIT"),
    ")"
);

pub fn cli_build_info() -> BuildInfo {
    BuildInfo::new(
        "track-cli",
        env!("CARGO_PKG_VERSION"),
        env!("TRACK_GIT_COMMIT"),
    )
}
