use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn make_fake_llama_completion(directory: &TempDir) -> std::path::PathBuf {
    let script_path = directory.path().join("llama-completion");
    fs::write(
        &script_path,
        r#"#!/bin/sh
HAS_SYSTEM_PROMPT=0
HAS_CONVERSATION=0
HAS_SINGLE_TURN=0
HAS_NO_DISPLAY_PROMPT=0
for ARG in "$@"; do
  [ "$ARG" = "-sys" ] && HAS_SYSTEM_PROMPT=1
  [ "$ARG" = "-cnv" ] && HAS_CONVERSATION=1
  [ "$ARG" = "--single-turn" ] && HAS_SINGLE_TURN=1
  [ "$ARG" = "--no-display-prompt" ] && HAS_NO_DISPLAY_PROMPT=1
done

if [ "$HAS_SYSTEM_PROMPT" -ne 1 ] || [ "$HAS_CONVERSATION" -ne 1 ] || [ "$HAS_SINGLE_TURN" -ne 1 ] || [ "$HAS_NO_DISPLAY_PROMPT" -ne 1 ]; then
  echo "expected llama-completion chat-style single-turn flags" >&2
  exit 7
fi

printf '%s\n' '{"project":"project-x","priority":"high","description":"Fix a bug in module A","confidence":"high"}'
"#,
    )
    .expect("fake llama-completion script should be written");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&script_path)
            .expect("fake llama-completion script should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions)
            .expect("fake llama-completion script should be executable");
    }

    script_path
}

#[test]
fn binary_creates_a_task_from_configured_local_parser() {
    let directory = TempDir::new().expect("tempdir should be created");
    let config_path = directory.path().join("config.json");
    let data_dir = directory.path().join("issues");
    let project_root = directory.path().join("projects");
    let fake_llama_completion = make_fake_llama_completion(&directory);

    fs::create_dir_all(project_root.join("project-x/.git"))
        .expect("fake git repository should be created");
    fs::write(
        &config_path,
        format!(
            r#"{{
  "projectRoots": ["{}"],
  "projectAliases": {{
    "proj-x": "project-x"
  }},
  "llamaCpp": {{
    "modelPath": "{}",
    "llamaCompletionPath": "{}"
  }}
}}
"#,
            project_root.display(),
            directory.path().join("parser.gguf").display(),
            fake_llama_completion.display(),
        ),
    )
    .expect("config file should be written");

    let mut command = Command::cargo_bin("track").expect("track binary should be available");
    let assert = command
        .env("TRACK_CONFIG_PATH", &config_path)
        .env("TRACK_DATA_DIR", &data_dir)
        .args(["proj-x", "fix", "a", "bug", "in", "module", "A"])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone())
        .expect("stdout should be valid utf-8");
    assert!(stdout.contains("Created task"));
    assert!(stdout.contains("Project"));
    assert!(stdout.contains("project-x"));
    assert!(stdout.contains("Priority"));
    assert!(stdout.contains("high"));

    let task_directory = data_dir.join("project-x/open");
    let entries = fs::read_dir(&task_directory)
        .expect("task directory should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("task directory entries should be readable");
    assert_eq!(entries.len(), 1);

    let raw_task = fs::read_to_string(entries[0].path()).expect("task file should be readable");
    assert!(raw_task.contains("Fix a bug in module A"));
    assert!(raw_task.contains("priority: high"));
    assert!(!raw_task.contains("project:"));
}
