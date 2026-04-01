use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    println!("cargo:rerun-if-env-changed=TRACK_GIT_COMMIT");

    if let Some(git_dir) = resolve_git_dir(&repo_root) {
        emit_git_rerun_hints(&git_dir);
    }

    let git_commit = read_injected_git_commit()
        .or_else(|| read_short_git_commit(&repo_root))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=TRACK_GIT_COMMIT={git_commit}");
}

// CI and Docker builds often do not include a `.git` directory in the build
// context. When the caller already knows which revision it is building, we let
// that injected value take priority and only shell out to `git` as a fallback.
fn read_injected_git_commit() -> Option<String> {
    normalize_git_commit(&std::env::var("TRACK_GIT_COMMIT").ok()?)
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
    normalize_git_commit(&commit)
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

fn normalize_git_commit(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.eq_ignore_ascii_case("unknown") {
        return Some("unknown".to_owned());
    }

    if trimmed
        .chars()
        .all(|character| character.is_ascii_hexdigit())
    {
        return Some(trimmed[..trimmed.len().min(7)].to_ascii_lowercase());
    }

    Some(trimmed.to_owned())
}
