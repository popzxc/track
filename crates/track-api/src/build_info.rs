use track_types::build_info::BuildInfo;

pub const SERVER_VERSION_TEXT: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("TRACK_GIT_COMMIT"),
    ")"
);

pub fn server_build_info() -> BuildInfo {
    BuildInfo::new(
        "track-api",
        env!("CARGO_PKG_VERSION"),
        env!("TRACK_GIT_COMMIT"),
    )
}
