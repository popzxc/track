use std::path::{Path, PathBuf};

pub mod api_harness;
pub mod fixture;

// =============================================================================
// Live-Test Gating
// =============================================================================
//
// These tests exercise Docker, SSH, and the local fixture tooling. They are
// intentionally part of the normal test graph so they keep compiling, but we
// only execute the heavy setup when the caller explicitly opts in.
//
// TODO: Replace this with a small shared test-runner utility if more crates
// start needing the same live-test gate.
pub fn live_integration_tests_enabled() -> bool {
    std::env::var("RUN_TRACK_INTEGRATION_TESTS")
        .map(|value| value == "true")
        .unwrap_or(false)
}

pub fn print_live_test_skip_message() {
    eprintln!("Skipped, set RUN_TRACK_INTEGRATION_TESTS to true to run tests");
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate directory should have a parent")
        .parent()
        .expect("workspace root should have a parent")
        .to_path_buf()
}
