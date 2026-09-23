#!/usr/bin/env python3
"""Regression checks for the indexed artifact/size gate; no compiler required."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "hygiene", Path(__file__).with_name("check-repo-hygiene.py"))
hygiene = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hygiene)


class HygieneTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)

    def stage(self, path, content):
        file = self.repo / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(content)
        subprocess.run(["git", "-C", str(self.repo), "add", "--", path], check=True)

    def test_empty_index_passes(self):
        self.assertEqual(hygiene.inspect_index(self.repo), [])

    def test_source_fixtures_and_archived_reports_pass(self):
        for path in ("src/target_info.rs", "tests/fixtures/output.txt",
                     "docs/archive/reports/model-catalog.md"):
            self.stage(path, b"intentional source or evidence")
        self.assertEqual(hygiene.inspect_index(self.repo), [])

    def test_generated_roots_and_reports_fail(self):
        paths = ("target/debug/aid", "target-custom/cache.bin", ".cargo-target/file",
                 "mutants.out/outcomes.json", "mutants.out.old/log", "output.txt",
                 "result.md", "result-task.md", "result_task.md")
        for path in paths:
            self.stage(path, b"generated")
        findings = hygiene.inspect_index(self.repo)
        for path in paths:
            self.assertIn("generated artifact is tracked: " + path, findings)

    def test_size_gate_reads_index_not_modified_worktree(self):
        self.stage("asset.bin", b"x" * 33)
        (self.repo / "asset.bin").write_bytes(b"small")
        self.assertEqual(hygiene.inspect_index(self.repo, max_bytes=32),
                         ["oversized blob (33 bytes): asset.bin"])

    def test_size_boundary_and_duplicate_blob_paths(self):
        self.stage("a.bin", b"x" * 32)
        self.stage("b.bin", b"x" * 32)
        self.assertEqual(hygiene.inspect_index(self.repo, max_bytes=32), [])
        self.assertEqual(len(hygiene.inspect_index(self.repo, max_bytes=31)), 2)

    def test_space_and_newline_paths_are_not_split(self):
        self.stage("docs/a b\nc.md", b"report")
        self.assertEqual(hygiene.inspect_index(self.repo), [])

    def test_cli_fails_for_tracked_output(self):
        self.stage("output.txt", b"generated")
        import sys
        result = subprocess.run(
            [sys.executable, str(Path(hygiene.__file__)), "--repo", str(self.repo)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("generated artifact is tracked: output.txt", result.stderr)


if __name__ == "__main__":
    unittest.main()
