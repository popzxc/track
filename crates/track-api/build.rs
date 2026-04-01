use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    if let Some(git_dir) = resolve_git_dir(&repo_root) {
        emit_git_rerun_hints(&git_dir);
    }

    let git_commit = read_short_git_commit(&repo_root).unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=TRACK_GIT_COMMIT={git_commit}");
}

fn read_short_git_commit(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let commit = String::from_utf8(output.stdout).ok()?;
    let commit = commit.trim();
    if commit.is_empty() {
        None
    } else {
        Some(commit.to_owned())
    }
}

fn resolve_git_dir(repo_root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let git_dir = String::from_utf8(output.stdout).ok()?;
    let git_dir = PathBuf::from(git_dir.trim());
    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(repo_root.join(git_dir))
    }
}

fn emit_git_rerun_hints(git_dir: &Path) {
    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());

    let Ok(head) = std::fs::read_to_string(&head_path) else {
        return;
    };
    let Some(reference) = head.strip_prefix("ref: ").map(str::trim) else {
        return;
    };

    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join(reference).display()
    );
}
