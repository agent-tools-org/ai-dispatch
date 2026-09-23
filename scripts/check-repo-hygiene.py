#!/usr/bin/env python3
"""Reject generated artifacts and oversized blobs in the Git index (Python 3.9+)."""
import argparse
import fnmatch
import subprocess
import sys


MAX_BLOB_BYTES = 5 * 1024 * 1024
GENERATED_ROOTS = {"target", ".cargo-target", "mutants.out", "mutants.out.old",
                   "nanobanana-output", "node_modules"}


def generated_path(path):
    root = path.split("/", 1)[0]
    if root in GENERATED_ROOTS or root.startswith("target-"):
        return True
    return "/" not in path and (
        path in {"output.txt", "result.md"}
        or fnmatch.fnmatchcase(path, "result-*.md")
        or fnmatch.fnmatchcase(path, "result_*.md")
    )


def inspect_index(repo, max_bytes=MAX_BLOB_BYTES):
    entries = subprocess.check_output(
        ["git", "-C", str(repo), "ls-files", "--stage", "-z"]
    ).split(b"\0")
    blobs = {}
    findings = []
    for entry in filter(None, entries):
        meta, raw_path = entry.split(b"\t", 1)
        mode, oid, stage = meta.split()
        path = raw_path.decode("utf-8", errors="surrogateescape")
        if stage != b"0":
            findings.append("unmerged index entry: " + path)
        if generated_path(path):
            findings.append("generated artifact is tracked: " + path)
        if mode != b"160000":  # Submodule commits are not file blobs.
            blobs.setdefault(oid, []).append(path)
    if blobs:
        result = subprocess.run(
            ["git", "-C", str(repo), "cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"],
            input=b"\n".join(blobs) + b"\n", stdout=subprocess.PIPE, check=True,
        )
        for line in result.stdout.splitlines():
            fields = line.split()
            if len(fields) != 3 or fields[1] != b"blob":
                raise RuntimeError("cannot inspect indexed object: " + line.decode())
            oid, _, size = fields
            if int(size) > max_bytes:
                for path in blobs[oid]:
                    findings.append("oversized blob ({} bytes): {}".format(int(size), path))
    return findings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default=".")
    args = parser.parse_args()
    findings = inspect_index(args.repo)
    if findings:
        print("Repository hygiene failed:", file=sys.stderr)
        for finding in findings:
            print("- " + finding, file=sys.stderr)
        print("Keep build outputs out of Git; put audit reports in docs/archive/reports/ "
              "and large binaries in release assets or an explicitly reviewed storage policy.",
              file=sys.stderr)
        return 1
    print("Repository hygiene passed (index paths and blob sizes).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
