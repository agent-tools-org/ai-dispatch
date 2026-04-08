#!/usr/bin/env bash
# Prepares an ai-dispatch release from a curated Markdown notes file.
# Runs tests, updates Cargo.toml and CHANGELOG.md, validates metadata, then commits/tags/pushes.
# Dependencies: bash, git, awk, grep, date, cargo.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dry_run="false"

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--dry-run] <version> <notes-file>

Arguments:
  version      Semantic version without the leading "v" (example: 8.75.0)
  notes-file   Markdown file containing release bullets, one per line

Example notes file:
  - Add release automation
  - Validate changelog before publish
EOF
}

fail() {
  echo "release failed: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

run_release_tests() {
  (cd "${repo_root}" && cargo test) || fail "cargo test failed"
}

package_version() {
  awk -F'"' '
    /^\[package\]/ { in_package = 1; next }
    /^\[/ && in_package { exit }
    in_package && $1 ~ /^version = / { print $2; exit }
  ' "${repo_root}/Cargo.toml"
}

validate_notes_file() {
  local notes_file="$1"
  [[ -f "${notes_file}" ]] || fail "notes file not found: ${notes_file}"
  grep -q '^- ' "${notes_file}" || fail "notes file must contain at least one Markdown bullet"
  if grep -Ev '^$|^-[[:space:]].+$' "${notes_file}" >/dev/null; then
    fail "notes file may only contain blank lines or '- ' bullets"
  fi
}

ensure_clean_worktree() {
  local status
  status="$(git -C "${repo_root}" status --short)"
  [[ -z "${status}" ]] || fail "git worktree must be clean before running release.sh"
}

ensure_branch_ready() {
  local branch
  branch="$(git -C "${repo_root}" rev-parse --abbrev-ref HEAD)"
  [[ "${branch}" != "HEAD" ]] || fail "detached HEAD is not supported"
  printf '%s' "${branch}"
}

ensure_tag_absent() {
  local tag="$1"
  if git -C "${repo_root}" rev-parse -q --verify "refs/tags/${tag}" >/dev/null 2>&1; then
    fail "tag already exists: ${tag}"
  fi
}

update_cargo_version() {
  local version="$1"
  local cargo_file="${repo_root}/Cargo.toml"
  local tmp_file
  tmp_file="$(mktemp "${TMPDIR:-/tmp}/cargo-release.XXXXXX")"
  awk -v version="${version}" '
    /^\[package\]/ {
      in_package = 1
      print
      next
    }
    /^\[/ && in_package && !done { in_package = 0 }
    in_package && $0 ~ /^version = "/ && !done {
      print "version = \"" version "\""
      done = 1
      next
    }
    { print }
    END { if (!done) exit 1 }
  ' "${cargo_file}" > "${tmp_file}" || fail "failed to update Cargo.toml version"
  mv "${tmp_file}" "${cargo_file}"
}

sync_cargo_lock() {
  (cd "${repo_root}" && cargo metadata --format-version 1 >/dev/null) \
    || fail "failed to sync Cargo.lock"
}

prepend_changelog_entry() {
  local version="$1"
  local notes_file="$2"
  local changelog_file="${repo_root}/CHANGELOG.md"
  local tmp_file
  local date_string
  tmp_file="$(mktemp "${TMPDIR:-/tmp}/changelog-release.XXXXXX")"
  date_string="$(date +%F)"
  {
    printf '## v%s (%s)\n' "${version}" "${date_string}"
    cat "${notes_file}"
    printf '\n\n'
    cat "${changelog_file}"
  } > "${tmp_file}"
  mv "${tmp_file}" "${changelog_file}"
}

main() {
  require_cmd git
  require_cmd awk
  require_cmd grep
  require_cmd date
  require_cmd cargo

  if [[ "${1:-}" == "--dry-run" ]]; then
    dry_run="true"
    shift
  fi

  [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]] && { usage; exit 0; }
  [[ $# -eq 2 ]] || { usage >&2; exit 1; }

  local version="$1"
  local notes_file="$2"
  local current_version
  local branch
  local tag
  local headline

  [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "version must look like X.Y.Z"
  validate_notes_file "${notes_file}"
  ensure_clean_worktree
  branch="$(ensure_branch_ready)"
  tag="v${version}"
  ensure_tag_absent "${tag}"

  current_version="$(package_version)"
  [[ -n "${current_version}" ]] || fail "could not read current package version"
  [[ "${current_version}" != "${version}" ]] || fail "Cargo.toml is already at version ${version}"

  run_release_tests
  update_cargo_version "${version}"
  sync_cargo_lock
  prepend_changelog_entry "${version}" "${notes_file}"
  bash "${repo_root}/.github/scripts/check-changelog.sh" "${tag}"

  headline="$(grep -m1 '^- ' "${notes_file}" | sed 's/^- //')"
  [[ -n "${headline}" ]] || fail "could not derive release headline"

  if [[ "${dry_run}" == "true" ]]; then
    echo "dry-run: updated Cargo.toml to ${version}"
    echo "dry-run: synchronized Cargo.lock"
    echo "dry-run: prepended CHANGELOG.md entry for ${tag}"
    echo "dry-run: would commit with message: feat: release ${tag} — ${headline}"
    echo "dry-run: would create tag ${tag}"
    echo "dry-run: would push branch ${branch} and tag ${tag}"
    exit 0
  fi

  git -C "${repo_root}" add Cargo.toml Cargo.lock CHANGELOG.md
  git -C "${repo_root}" commit -m "feat: release ${tag} — ${headline}"
  git -C "${repo_root}" tag "${tag}"
  git -C "${repo_root}" push origin "${branch}"
  git -C "${repo_root}" push origin "${tag}"
}

main "$@"
