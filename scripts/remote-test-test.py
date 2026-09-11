#!/usr/bin/env python3
# Unit-style remote-test command and diagnostic checks; never contacts a box or runs cargo.
# Entry: python3 scripts/remote-test-test.py; dependencies: Python stdlib, bash, git.
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/remote-test.sh"


class RemoteTestCommandTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="remote-test-proof-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.repo = self.base / "sample repo"
        self.repo.mkdir()
        self.env = dict(os.environ)
        for key in list(self.env):
            if key.startswith("GIT_"):
                del self.env[key]
        self.env.update(AID_BUILD_BOX="fixture", AID_BUILD_JOBS="4")
        self.git("init", "-b", "release/ready")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "Fixture")
        self.bin = self.base / "bin"
        self.bin.mkdir()
        self.capture = self.base / "args.jsonl"
        self.env.update(PATH=f"{self.bin}:{self.env['PATH']}", CAPTURE=str(self.capture))
        fake = self.bin / "rbox"
        fake.write_text("""#!/usr/bin/env python3
import json, os, sys
with open(os.environ['CAPTURE'], 'a') as capture:
    capture.write(json.dumps(sys.argv[1:]) + '\\n')
if os.environ.get('FAKE_JOB', 'yes') == 'yes':
    print('rbox: job fixture-job', file=sys.stderr, flush=True)
status = int(os.environ.get('FAKE_STATUS', '0'))
if os.environ.get('FAKE_COMPLETE') == 'yes':
    print(f'rbox: job fixture-job exited with code {status}', flush=True)
sys.exit(status)
""")
        fake.chmod(0o755)

    def git(self, *args: str) -> str:
        return subprocess.check_output(
            ["git", "-C", str(self.repo), *args], env=self.env,
            text=True, stderr=subprocess.PIPE,
        ).strip()

    def run_script(self, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["/bin/bash", str(SCRIPT), *args], cwd=cwd or self.repo,
            env=self.env, text=True, capture_output=True,
        )

    def dry_command(self, *args: str, cwd: Path | None = None) -> list[str]:
        result = self.run_script("--dry-run", *args, cwd=cwd)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.capture.exists(), "dry-run must not invoke rbox")
        replay = subprocess.run(
            ["/bin/bash", "-c", result.stdout], env=self.env, capture_output=True, text=True,
        )
        self.assertEqual(replay.returncode, 0, replay.stderr)
        calls = self.capture.read_text().splitlines()
        self.assertEqual(len(calls), 1)
        self.capture.unlink()
        return json.loads(calls[0])

    def test_branch_defaults_and_remote_home(self) -> None:
        args = self.dry_command()
        self.assertEqual(args, [
            "exec", "fixture", str(self.repo), "--to",
            "~/.rbox/work/sample-repo/release-ready", "--jobs", "4",
            "--timeout", "5400", "--lock-timeout", "3600", "--", "bash", "-c",
            'export CARGO_TARGET_DIR=$HOME/.rbox/target/sample-repo; '
            'exec cargo test --workspace "$@"', "remote-test",
        ])

    def test_detached_head_uses_short_sha(self) -> None:
        self.git("checkout", "--detach")
        args = self.dry_command()
        self.assertEqual(args[4], f"~/.rbox/work/sample-repo/{self.git('rev-parse', '--short', 'HEAD')}")

    def test_linked_worktree_shares_repo_target_and_accepts_subdirectory(self) -> None:
        worktree = self.base / "task worktree"
        self.git("worktree", "add", "-b", "fix/task", str(worktree))
        nested = worktree / "nested"
        nested.mkdir()
        args = self.dry_command(cwd=nested)
        self.assertEqual(args[2], str(worktree))
        self.assertEqual(args[4], "~/.rbox/work/sample-repo/fix-task")
        self.assertIn("$HOME/.rbox/target/sample-repo", args[14])

    def test_options_and_extra_arguments_remain_literal(self) -> None:
        extra = ["--bin", "aid", "", "a filter", "$(touch sentinel)", "'quoted'", "--", "--exact"]
        args = self.dry_command("--jobs", "2", "--timeout", "9", "--lock-timeout", "0", "--", *extra)
        self.assertEqual(args[5:11], ["--jobs", "2", "--timeout", "9", "--lock-timeout", "0"])
        self.assertEqual(args[16:], extra)
        self.assertFalse((self.repo / "sentinel").exists())

    def test_jobs_environment_default(self) -> None:
        self.env["AID_BUILD_JOBS"] = "3"
        self.assertEqual(self.dry_command()[6], "3")

    def test_unset_box_fails_before_rbox(self) -> None:
        del self.env["AID_BUILD_BOX"]
        result = self.run_script("--dry-run")
        self.assertEqual(result.returncode, 2)
        self.assertIn("AID_BUILD_BOX is not set", result.stderr)
        self.assertFalse(self.capture.exists())

    def test_missing_option_values_are_clear(self) -> None:
        for option in ["--jobs", "--timeout", "--lock-timeout"]:
            with self.subTest(option=option):
                result = self.run_script(option)
                self.assertEqual(result.returncode, 2)
                self.assertIn(f"missing value for {option}", result.stderr)

    def test_timeout_names_job_and_preserves_status(self) -> None:
        self.env["FAKE_STATUS"] = "124"
        result = self.run_script()
        self.assertEqual(result.returncode, 124)
        self.assertIn("job fixture-job: timed out after 5400s", result.stderr)
        self.assertIn("job continues on the box", result.stderr)
        self.assertEqual(len(self.capture.read_text().splitlines()), 1)

    def test_lock_timeout_names_job_when_available(self) -> None:
        self.env["FAKE_STATUS"] = "75"
        result = self.run_script()
        self.assertEqual(result.returncode, 75)
        self.assertIn("job fixture-job: box lock not acquired within 3600s", result.stderr)
        self.assertNotIn("timed out", result.stderr)

    def test_sync_lock_failure_has_no_fabricated_job(self) -> None:
        self.env.update(FAKE_STATUS="75", FAKE_JOB="no")
        result = self.run_script()
        self.assertEqual(result.returncode, 75)
        self.assertIn("job not started (no job id assigned)", result.stderr)
        self.assertIn("box lock not acquired", result.stderr)

    def test_completed_test_status_is_not_misreported_as_timeout(self) -> None:
        for status in [0, 1, 75, 124]:
            with self.subTest(status=status):
                self.env.update(FAKE_STATUS=str(status), FAKE_COMPLETE="yes")
                result = self.run_script()
                self.assertEqual(result.returncode, status)
                self.assertIn(f"exited with code {status}", result.stdout)
                self.assertNotIn("timed out", result.stderr)
                self.assertNotIn("box lock not acquired", result.stderr)
                if status:
                    self.assertIn(f"job fixture-job: tests failed (exit {status})", result.stderr)

    def test_verify_and_guide_command_coverage(self) -> None:
        config = (ROOT / ".aid/project.toml").read_text()
        self.assertIn('verify = "scripts/remote-test.sh -- --bin aid"', config)
        guide = (ROOT / "default-skills/aid-guide/references/configuration.md").read_text()
        release = (ROOT / "CLAUDE.md").read_text()
        for text in [guide, release]:
            for contract in [
                "scripts/remote-test.sh -- --bin aid", "AID_RELEASE_TEST_CMD='scripts/remote-test.sh'",
                "AID_BUILD_BOX", "RBOX_CONFIG", "PATH", "one `rbox exec`", "non-root",
                "~/.rbox/work/<repo-name>/<checkout-id>", "$HOME/.rbox/target/<repo-name>",
                "--jobs N", "--timeout S", "--lock-timeout S", "--dry-run",
                "Exit 124", "75 means", "no test skips", "outside the", "agent sandbox",
            ]:
                self.assertIn(contract, text)
        self.assertNotIn("--skip", SCRIPT.read_text())


if __name__ == "__main__":
    unittest.main(verbosity=2)
