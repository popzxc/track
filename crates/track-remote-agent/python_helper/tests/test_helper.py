import json
import os
import stat
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
import zipapp
from pathlib import Path

from track_remote_helper.commands import read_run_snapshots, write_file
from track_remote_helper.common import (
    CODEX_EVENTS_FILE_NAME,
    FINISHED_AT_FILE_NAME,
    PROMPT_FILE_NAME,
    RESULT_FILE_NAME,
    SCHEMA_FILE_NAME,
    STATUS_FILE_NAME,
)
from track_remote_helper.worker import run_worker_from_config


class RemoteHelperTests(unittest.TestCase):
    def test_write_file_creates_parent_directories(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            target_path = Path(temp_dir) / "nested" / "remote" / "prompt.md"

            write_file({"path": str(target_path), "contents": "# prompt\n"})

            self.assertEqual(target_path.read_text(encoding="utf-8"), "# prompt\n")

    def test_read_run_snapshots_reports_present_and_missing_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            run_directory = Path(temp_dir) / "dispatch-1"
            run_directory.mkdir(parents=True)
            (run_directory / STATUS_FILE_NAME).write_text("running\n", encoding="utf-8")
            (run_directory / RESULT_FILE_NAME).write_text('{"status":"ok"}\n', encoding="utf-8")

            response = read_run_snapshots({"runDirectories": [str(run_directory)]})

            self.assertEqual(len(response["snapshots"]), 1)
            snapshot = response["snapshots"][0]
            self.assertEqual(snapshot["runDirectory"], str(run_directory))
            self.assertEqual(snapshot["status"], "running\n")
            self.assertEqual(snapshot["result"], '{"status":"ok"}\n')
            self.assertIsNone(snapshot["stderr"])
            self.assertIsNone(snapshot["finishedAt"])

    def test_worker_runs_tool_with_shell_prelude_and_persists_result_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            run_directory = temp_root / "dispatch-1"
            worktree_path = temp_root / "worktree"
            fake_codex_path = temp_root / "fake-codex.py"
            config_path = run_directory / "worker-config.json"

            run_directory.mkdir(parents=True)
            worktree_path.mkdir(parents=True)
            (run_directory / PROMPT_FILE_NAME).write_text("# prompt\n", encoding="utf-8")
            (run_directory / SCHEMA_FILE_NAME).write_text('{"type":"object"}\n', encoding="utf-8")
            write_executable(
                fake_codex_path,
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import json
                    import os
                    import sys
                    from pathlib import Path

                    output_path = None
                    index = 0
                    while index < len(sys.argv):
                        if sys.argv[index] == "-o":
                            output_path = Path(sys.argv[index + 1])
                            break
                        index += 1

                    if output_path is None:
                        raise SystemExit("missing -o argument")

                    prompt = sys.stdin.read()
                    output_path.write_text(
                        json.dumps(
                            {
                                "status": "succeeded",
                                "prompt": prompt.strip(),
                                "prelude": os.environ.get("TRACK_REMOTE_HELPER_PRELUDE_TEST"),
                            }
                        ),
                        encoding="utf-8",
                    )
                    print(json.dumps({"event": "worker-test"}))
                    """
                ),
            )
            config_path.write_text(
                json.dumps(
                    {
                        "preferredTool": "codex",
                        "runDirectory": str(run_directory),
                        "shellPrelude": 'export TRACK_REMOTE_HELPER_PRELUDE_TEST="from-prelude"',
                        "worktreePath": str(worktree_path),
                    }
                ),
                encoding="utf-8",
            )

            previous_override = os.environ.get("TRACK_REMOTE_HELPER_CODEX")
            os.environ["TRACK_REMOTE_HELPER_CODEX"] = str(fake_codex_path)
            try:
                exit_code = run_worker_from_config(config_path)
            finally:
                if previous_override is None:
                    os.environ.pop("TRACK_REMOTE_HELPER_CODEX", None)
                else:
                    os.environ["TRACK_REMOTE_HELPER_CODEX"] = previous_override

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                (run_directory / STATUS_FILE_NAME).read_text(encoding="utf-8").strip(),
                "completed",
            )
            result = json.loads((run_directory / RESULT_FILE_NAME).read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "succeeded")
            self.assertEqual(result["prompt"], "# prompt")
            self.assertEqual(result["prelude"], "from-prelude")
            self.assertIn("worker-test", (run_directory / CODEX_EVENTS_FILE_NAME).read_text(encoding="utf-8"))
            self.assertTrue((run_directory / FINISHED_AT_FILE_NAME).is_file())

    def test_zipapp_launch_run_reexecs_the_packaged_helper_for_the_worker(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            helper_root = Path(__file__).resolve().parents[1]
            zipapp_path = temp_root / "track-remote-helper.pyz"
            run_directory = temp_root / "dispatch-1"
            worktree_path = temp_root / "worktree"
            fake_codex_path = temp_root / "fake-codex.py"

            zipapp.create_archive(
                helper_root,
                zipapp_path,
                main="track_remote_helper.__main__:main",
            )

            run_directory.mkdir(parents=True)
            worktree_path.mkdir(parents=True)
            (run_directory / PROMPT_FILE_NAME).write_text("# prompt\n", encoding="utf-8")
            (run_directory / SCHEMA_FILE_NAME).write_text('{"type":"object"}\n', encoding="utf-8")
            write_executable(
                fake_codex_path,
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import json
                    import sys
                    from pathlib import Path

                    output_path = None
                    index = 0
                    while index < len(sys.argv):
                        if sys.argv[index] == "-o":
                            output_path = Path(sys.argv[index + 1])
                            break
                        index += 1

                    if output_path is None:
                        raise SystemExit("missing -o argument")

                    output_path.write_text(
                        json.dumps({"status": "succeeded", "summary": "zipapp worker ok"}),
                        encoding="utf-8",
                    )
                    print(json.dumps({"event": "zipapp-worker-test"}))
                    """
                ),
            )

            env = os.environ.copy()
            env["TRACK_REMOTE_HELPER_CODEX"] = str(fake_codex_path)
            launch_completed = subprocess.run(
                [sys.executable, str(zipapp_path), "launch-run"],
                input=json.dumps(
                    {
                        "runDirectory": str(run_directory),
                        "worktreePath": str(worktree_path),
                        "preferredTool": "codex",
                    }
                ),
                capture_output=True,
                check=True,
                env=env,
                text=True,
            )
            self.assertEqual(launch_completed.stdout.strip(), "{}")

            for _ in range(50):
                status_path = run_directory / STATUS_FILE_NAME
                if status_path.exists():
                    current_status = status_path.read_text(encoding="utf-8").strip()
                    if current_status in {"completed", "launcher_failed", "canceled"}:
                        break
                else:
                    current_status = ""
                time.sleep(0.1)
            else:
                self.fail("zipapp launch-run worker did not reach a terminal status")

            self.assertEqual(current_status, "completed")
            self.assertEqual(
                json.loads((run_directory / RESULT_FILE_NAME).read_text(encoding="utf-8")),
                {"status": "succeeded", "summary": "zipapp worker ok"},
            )
            self.assertIn(
                "zipapp-worker-test",
                (run_directory / CODEX_EVENTS_FILE_NAME).read_text(encoding="utf-8"),
            )


def write_executable(path: Path, contents: str) -> None:
    path.write_text(contents, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


if __name__ == "__main__":
    unittest.main()
