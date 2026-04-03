// Phase 1 keeps the moved CLI support modules in `track-cli` even where the
// current command flow still goes through older wrappers in `cli.rs`.
#[allow(dead_code)]
mod api_notify;
mod backend_client;
mod build_info;
mod cli_config;
#[allow(dead_code)]
mod terminal_ui;
#[allow(dead_code)]
mod wizard;

#[cfg(test)]
mod test_support {
    pub use track_types::test_support::*;
}

pub mod cli;
