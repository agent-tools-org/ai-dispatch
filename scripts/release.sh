#!/usr/bin/env bash
# Prepares an ai-dispatch release from a curated Markdown notes file.
# Runs tests, updates Cargo.toml and CHANGELOG.md, validates metadata, then commits/tags/pushes.
# Dependencies: bash, git, awk, grep, date, cargo.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dry_run="false"
skip_hygiene="false"

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--dry-run] [--skip-hygiene] <version> <notes-file>

Options:
  --dry-run       Validate without committing or pushing; hygiene failures still exit non-zero
  --skip-hygiene  Warn about orphan branches/worktrees without failing

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
  # Compiles belong on the build box: set AID_RELEASE_TEST_CMD (for example
  # `scripts/remote-test.sh` with AID_BUILD_BOX) to run the suite there. The
  # default keeps a plain local `cargo test` for machines without a box.
  local cmd="${AID_RELEASE_TEST_CMD:-cargo test}"
  (cd "${repo_root}" && bash -c "${cmd}") || fail "release tests failed: ${cmd}"
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

# Returns 0 when a branch should be excluded from orphan branch cleanup.
branch_is_kept() {
  local branch="$1"
  local current_branch="$2"
  [[ "${branch}" == "main" || "${branch}" == "gitbutler/workspace" || "${branch}" == "${current_branch}" || "${branch}" == keep/* ]]
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

# Prints the orphan hygiene report and suggested cleanup commands to stderr.
print_orphan_report() {
  local orphan_worktrees="$1" orphan_branches="$2" line path ids id db="${HOME}/.aid/aid.db"
  printf 'orphan hygiene check found cleanup candidates\n' >&2
  if [[ -n "${orphan_worktrees}" ]]; then
    printf '\nOrphan worktrees:\n' >&2
    while IFS= read -r line; do
      [[ -n "${line}" ]] || continue
      printf '  - %s\n' "${line}" >&2
      path="${line% (*}"
      ids=""
      if command -v sqlite3 >/dev/null 2>&1 && [[ -f "${db}" ]]; then
        ids="$(sqlite3 "${db}" "select id from tasks where worktree_path = '${path//\'/\'\'}' order by created_at desc" 2>/dev/null || true)"
      fi
      while IFS= read -r id; do
        [[ -n "${id}" ]] || continue
        printf '    aid accept %s\n    aid gc --task %s\n' "${id}" "${id}" >&2
      done <<< "${ids}"
    done <<< "${orphan_worktrees}"
  fi
  if [[ -n "${orphan_branches}" ]]; then
    printf '\nOrphan branches:\n' >&2
    while IFS= read -r line; do
      [[ -n "${line}" ]] && printf '  - %s\n' "${line}" >&2
    done <<< "${orphan_branches}"
  fi
  printf '\nTask artifacts require explicit principal acceptance and custody GC.\n' >&2
}

# Fails on merged-orphan branches or worktrees unless hygiene checks are skipped.
check_orphans() {
  local current_branch merged_output line branch branch_ref worktree_path n=0 m=0
  local orphan_branches="" orphan_worktrees="" merged_names=""
  current_branch="$(ensure_branch_ready)"

  merged_output="$(git -C "${repo_root}" branch --merged main)"

  while IFS= read -r line; do
    branch="${line#\* }"
    branch="${branch#+ }"
    branch="${branch#"${branch%%[![:space:]]*}"}"
    [[ -n "${branch}" ]] || continue
    merged_names+="${branch}"$'\n'
    branch_is_kept "${branch}" "${current_branch}" && continue
    orphan_branches+="${branch}"$'\n'
    n=$((n + 1))
  done <<< "${merged_output}"

  while IFS= read -r line; do
    case "${line}" in
      worktree\ *)
        worktree_path="${line#worktree }"
        branch_ref=""
        ;;
      branch\ refs/heads/*)
        branch_ref="${line#branch refs/heads/}"
        ;;
      '')
        [[ -n "${worktree_path}" && "${worktree_path}" != "${repo_root}" ]] || { worktree_path=""; continue; }
        if [[ ! -d "${worktree_path}" ]]; then
          orphan_worktrees+="${worktree_path} (missing path)"$'\n'
          m=$((m + 1))
        elif printf '%s\n' "${merged_names}" | grep -Fqx "${branch_ref}"; then
          orphan_worktrees+="${worktree_path} (${branch_ref})"$'\n'
          m=$((m + 1))
        fi
        worktree_path=""
        ;;
    esac
  done < <(git -C "${repo_root}" worktree list --porcelain; printf '\n')

  [[ -z "${orphan_branches}${orphan_worktrees}" ]] && return 0

  print_orphan_report "${orphan_worktrees}" "${orphan_branches}"
  [[ "${skip_hygiene}" == "true" ]] && return 0
  if [[ "${dry_run}" == "true" ]]; then
    printf 'dry-run: would fail: release hygiene check (%s orphan branches, %s orphan worktrees)\n' "${n}" "${m}" >&2
    exit 1
  fi
  fail "release hygiene check failed"
}

main() {
  require_cmd git
  require_cmd awk
  require_cmd grep
  require_cmd date
  require_cmd cargo

  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --dry-run)
        dry_run="true"
        shift
        ;;
      --skip-hygiene)
        skip_hygiene="true"
        shift
        ;;
      *)
        break
        ;;
    esac
  done

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
  check_orphans
  branch="$(ensure_branch_ready)"
  tag="v${version}"
  ensure_tag_absent "${tag}"

  current_version="$(package_version)"
  [[ -n "${current_version}" ]] || fail "could not read current package version"
  [[ "${current_version}" != "${version}" ]] || fail "Cargo.toml is already at version ${version}"

  # Dry-run must leave the worktree exactly as it found it, even if a later
  # validation step fails. The worktree is verified clean above, so a plain
  # checkout restores the pre-run state.
  if [[ "${dry_run}" == "true" ]]; then
    trap 'git -C "${repo_root}" checkout -- Cargo.toml Cargo.lock CHANGELOG.md' EXIT
  fi

  run_release_tests
  update_cargo_version "${version}"
  sync_cargo_lock
  prepend_changelog_entry "${version}" "${notes_file}"
  bash "${repo_root}/.github/scripts/check-changelog.sh" "${tag}"

  headline="$(grep -m1 '^- ' "${notes_file}" | sed 's/^- //')"
  [[ -n "${headline}" ]] || fail "could not derive release headline"

  if [[ "${dry_run}" == "true" ]]; then
    echo "dry-run: validated Cargo.toml bump to ${version} (reverted)"
    echo "dry-run: validated Cargo.lock sync (reverted)"
    echo "dry-run: validated CHANGELOG.md entry for ${tag} (reverted)"
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
