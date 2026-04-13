//! Shared shell utilities that remain relevant after the helper migration.
//!
//! The embedded Python helper now owns the remote operation surface, but the
//! SSH transport still needs two tiny shell capabilities:
//!
//! 1. expand home-relative paths predictably during bootstrap
//! 2. prepend the user-configured shell prelude before a bootstrap snippet
//!
//! Keeping those pieces here avoids spreading ad hoc shell assembly through the
//! transport layer while the rest of the historical script code stays out of
//! the normal build.

use std::borrow::Cow;

use serde::Serialize;

use crate::template_renderer::render_template;

const REMOTE_SCRIPT_WRAPPER_TEMPLATE: &str =
    include_str!("../../templates/scripts/remote_script_wrapper.sh.tera");

/// Returns the shared shell function used by remote scripts to expand
/// home-relative paths in a predictable, non-interactive way.
///
/// Remote automation cannot rely on shell-specific tilde expansion rules once
/// paths start flowing through quoted variables and script arguments. This
/// helper gives every script the same stable path contract.
pub(crate) fn remote_path_helpers_shell() -> &'static str {
    r#"
expand_remote_path() {
  case "$1" in
    "~")
      printf '%s\n' "$HOME"
      ;;
    "~/"*)
      # Strip the literal `~/` prefix before joining with $HOME. Bash expands
      # an unescaped `~` inside `${var#pattern}` to the current home path,
      # which leaves the original `~/...` intact and produces `$HOME/~/...`.
      printf '%s/%s\n' "$HOME" "${1#\~/}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}
"#
}

pub(crate) fn render_remote_script_with_shell_prelude(script: &str, shell_prelude: &str) -> String {
    let normalized_shell_prelude = normalize_shell_prelude(shell_prelude);
    let template_context = RemoteScriptWrapperTemplate {
        shell_prelude: normalized_shell_prelude.as_ref(),
        script_body: script.trim_start_matches('\n'),
    };

    render_template(REMOTE_SCRIPT_WRAPPER_TEMPLATE, &template_context)
}

#[derive(Serialize)]
struct RemoteScriptWrapperTemplate<'a> {
    shell_prelude: &'a str,
    script_body: &'a str,
}

fn normalize_shell_prelude(shell_prelude: &str) -> Cow<'_, str> {
    if shell_prelude.trim().is_empty() {
        Cow::Borrowed("")
    } else if shell_prelude.ends_with('\n') {
        Cow::Borrowed(shell_prelude)
    } else {
        Cow::Owned(format!("{shell_prelude}\n"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prepends_shell_prelude_before_remote_script_body() {
        let rendered = super::render_remote_script_with_shell_prelude(
            "set -eu\nprintf '%s\\n' done\n",
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"\n",
        );

        assert!(rendered.starts_with("set -e\n"));
        assert!(rendered.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(rendered.contains(". \"$HOME/.cargo/env\""));
        assert!(rendered.contains("printf '%s\\n' done"));
    }

    #[test]
    fn expands_tilde_prefixed_remote_paths_without_reintroducing_a_literal_tilde_segment() {
        let helper_script = format!(
            r#"
set -eu
HOME="/tmp/remote-home"
{path_helpers}
expand_remote_path "$1"
"#,
            path_helpers = super::remote_path_helpers_shell(),
        );

        let output = std::process::Command::new("bash")
            .arg("-s")
            .arg("--")
            .arg("~/workspace/project-a/dispatches/dispatch-1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;

                let stdin = child.stdin.as_mut().expect("bash stdin should exist");
                stdin
                    .write_all(helper_script.as_bytes())
                    .expect("helper script should write to bash stdin");
                child.wait_with_output()
            })
            .expect("bash helper should run");

        assert!(
            output.status.success(),
            "helper script should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "/tmp/remote-home/workspace/project-a/dispatches/dispatch-1"
        );
    }
}
