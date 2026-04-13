use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let helper_root = manifest_dir.join("python_helper");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR should exist"));
    let helper_output = out_dir.join("track-remote-helper.pyz");

    emit_rerun_hints(&helper_root);
    build_python_helper_zipapp(&helper_root, &helper_output);

    let helper_bytes =
        fs::read(&helper_output).expect("the packaged remote helper should be readable");
    let helper_version = helper_version(&helper_bytes);
    println!("cargo:rustc-env=TRACK_REMOTE_HELPER_VERSION={helper_version}");
}

fn emit_rerun_hints(root: &Path) {
    println!("cargo:rerun-if-changed={}", root.display());

    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();
            println!("cargo:rerun-if-changed={}", entry_path.display());
            if entry_path.is_dir() {
                stack.push(entry_path);
            }
        }
    }
}

fn build_python_helper_zipapp(helper_root: &Path, helper_output: &Path) {
    let status = Command::new("python3")
        .arg("-m")
        .arg("zipapp")
        .arg(helper_root)
        .arg("-o")
        .arg(helper_output)
        .arg("-m")
        .arg("track_remote_helper.__main__:main")
        .status()
        .unwrap_or_else(|error| {
            panic!("failed to start `python3 -m zipapp` while packaging the remote helper: {error}")
        });

    if !status.success() {
        panic!(
            "`python3 -m zipapp` failed while packaging the remote helper at {}",
            helper_root.display()
        );
    }
}

fn helper_version(helper_bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    helper_bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
