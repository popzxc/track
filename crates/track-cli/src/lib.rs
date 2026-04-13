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
    use std::path::Path;
    use std::process::Command;

    pub use track_types::test_support::*;

    pub fn run_git(checkout_path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(checkout_path)
            .args(args)
            .output()
            .expect("git command should run");

        assert!(
            output.status.success(),
            "git {:?} should succeed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn create_git_checkout(checkout_path: &Path, origin_url: &str) {
        std::fs::create_dir_all(checkout_path).expect("checkout path should exist");
        run_git(checkout_path, &["init"]);
        run_git(checkout_path, &["config", "user.name", "track-tests"]);
        run_git(
            checkout_path,
            &["config", "user.email", "track@example.com"],
        );
        std::fs::write(checkout_path.join("README.md"), "hello\n")
            .expect("fixture file should exist");
        run_git(checkout_path, &["add", "README.md"]);
        run_git(checkout_path, &["commit", "-m", "Initial commit"]);
        run_git(checkout_path, &["branch", "-M", "main"]);
        run_git(checkout_path, &["remote", "add", "origin", origin_url]);
    }
}

pub mod cli;
