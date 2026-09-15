#!/usr/bin/env bash
# Orphan-hygiene helpers sourced by scripts/release.sh.
# Exports: branch_is_kept, print_orphan_report, check_orphans.
# Dependencies: git, sqlite3 (optional); uses repo_root, dry_run, skip_hygiene, fail.

# Returns 0 when a branch should be excluded from orphan branch cleanup.
branch_is_kept() {
  local branch="$1"
  local current_branch="$2"
  [[ "${branch}" == "main" || "${branch}" == "gitbutler/workspace" || "${branch}" == "${current_branch}" || "${branch}" == keep/* ]]
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

# Physical directory path via cd -P / pwd -P (no realpath dependency).
canonical_dir() {
  (cd -P -- "$1" && pwd -P)
}

# Returns 0 when wt/.aid-lock names a live pid. Sets held_task_id from the lock.
worktree_held() {
  local lock="$1/.aid-lock" k v live=1 line
  held_task_id=""
  [[ -f "${lock}" ]] || return 1
  while IFS= read -r line || [[ -n "${line}" ]]; do
    k="${line%%=*}"
    v="${line#*=}"
    case "${k}" in
      task_id) held_task_id="${v}" ;;
      owner_pid|worker_pid)
        [[ "${v}" =~ ^[0-9]+$ ]] && kill -0 "${v}" 2>/dev/null && live=0
        ;;
    esac
  done < "${lock}"
  return "${live}"
}

# Fails on merged-orphan branches or worktrees unless hygiene checks are skipped.
check_orphans() {
  local current_branch merged_output line branch branch_ref worktree_path n=0 m=0
  local orphan_branches="" orphan_worktrees="" merged_names="" held_worktrees="" held_branches=""
  local repo_root_canon wt_canon held_line held_task_id
  current_branch="$(ensure_branch_ready)"
  repo_root_canon="$(canonical_dir "${repo_root}")"

  merged_output="$(git -C "${repo_root}" branch --merged main)"

  while IFS= read -r line; do
    branch="${line#\* }"
    branch="${branch#+ }"
    branch="${branch#"${branch%%[![:space:]]*}"}"
    [[ -n "${branch}" ]] || continue
    merged_names+="${branch}"$'\n'
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
        [[ -n "${worktree_path}" ]] || continue
        wt_canon="$(canonical_dir "${worktree_path}" 2>/dev/null)" || wt_canon="${worktree_path}"
        if [[ "${wt_canon}" == "${repo_root_canon}" ]]; then
          worktree_path=""
          continue
        fi
        if worktree_held "${worktree_path}"; then
          held_line="${worktree_path} (${branch_ref})"
          [[ -n "${held_task_id}" ]] && held_line+=" ${held_task_id}"
          held_worktrees+="${held_line}"$'\n'
          [[ -n "${branch_ref}" ]] && held_branches+="${branch_ref}"$'\n'
        elif [[ ! -d "${worktree_path}" ]]; then
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

  while IFS= read -r branch; do
    [[ -n "${branch}" ]] || continue
    branch_is_kept "${branch}" "${current_branch}" && continue
    if [[ -n "${held_branches}" ]] && printf '%s\n' "${held_branches}" | grep -Fqx "${branch}"; then
      continue
    fi
    orphan_branches+="${branch}"$'\n'
    n=$((n + 1))
  done <<< "${merged_names}"

  if [[ -n "${held_worktrees}" ]]; then
    printf 'Held by running tasks (not orphans):\n' >&2
    while IFS= read -r line; do
      [[ -n "${line}" ]] && printf '  - %s\n' "${line}" >&2
    done <<< "${held_worktrees}"
    [[ -n "${orphan_branches}${orphan_worktrees}" ]] && printf '\n' >&2
  fi

  [[ -z "${orphan_branches}${orphan_worktrees}" ]] && return 0

  print_orphan_report "${orphan_worktrees}" "${orphan_branches}"
  [[ "${skip_hygiene}" == "true" ]] && return 0
  if [[ "${dry_run}" == "true" ]]; then
    printf 'dry-run: would fail: release hygiene check (%s orphan branches, %s orphan worktrees)\n' "${n}" "${m}" >&2
    exit 1
  fi
  fail "release hygiene check failed"
}
