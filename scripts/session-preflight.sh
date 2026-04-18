#!/usr/bin/env bash
# Session-start preflight: survey repo state so Claude Code sees the real situation.
# Runs from Claude Code's SessionStart hook. Output is short and goes to the session as context.

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

# Only run in git repos — hook is wired at repo level but defend anyway.
git rev-parse --git-dir >/dev/null 2>&1 || { echo "[preflight] not a git repo — skipping"; exit 0; }

sev="ok"
bump_sev() { local new="$1"; case "${sev}/${new}" in */crit) sev="crit";; */warn) [[ "${sev}" == "ok" ]] && sev="warn";; esac; }

header_line() {
  case "${sev}" in
    crit) echo "[preflight] CRITICAL — read findings before acting";;
    warn) echo "[preflight] warnings — review before long work";;
    ok)   echo "[preflight] repo is clean and current";;
  esac
}

# ── fetch (best-effort, 5s timeout) ─────────────────────────────────────────
fetch_note=""
if command -v timeout >/dev/null 2>&1; then
  if ! timeout 5 git fetch --quiet origin 2>/dev/null; then
    fetch_note="  fetch: skipped (offline or slow)"
    bump_sev warn
  fi
else
  git fetch --quiet origin 2>/dev/null || fetch_note="  fetch: skipped (offline)"
fi

# ── branch + ahead/behind vs origin/main ────────────────────────────────────
branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "detached")"
base="origin/main"
git show-ref --verify --quiet "refs/remotes/${base}" || base="origin/master"

aheadbehind=""
behind=0
if git show-ref --verify --quiet "refs/remotes/${base}"; then
  counts="$(git rev-list --left-right --count "${base}...HEAD" 2>/dev/null || echo "0 0")"
  behind="${counts%$'\t'*}"
  behind="${behind%% *}"
  ahead="${counts##*$'\t'}"
  ahead="${ahead##* }"
  aheadbehind="  branch: ${branch} (ahead ${ahead}, behind ${behind} vs ${base})"
  if (( behind > 20 )); then
    aheadbehind+=" ⚠️  SIGNIFICANTLY stale — rebase or start fresh before work"
    bump_sev crit
  elif (( behind > 5 )); then
    aheadbehind+=" ⚠️  stale"
    bump_sev warn
  fi
else
  aheadbehind="  branch: ${branch} (no ${base} remote to compare)"
fi

# ── working dir clean? ──────────────────────────────────────────────────────
dirty=""
dirty_count="$(git status --porcelain=v1 2>/dev/null | wc -l | tr -d ' ')"
if (( dirty_count > 0 )); then
  dirty="  dirty: ${dirty_count} file(s) modified/untracked"
  if (( dirty_count > 20 )); then
    dirty+=" — likely leaked artifacts from a prior session"
    bump_sev warn
  fi
fi

# ── aid zombie tasks (running but PID dead) ────────────────────────────────
zombie_note=""
if [[ -f ~/.aid/aid.db ]] && command -v sqlite3 >/dev/null 2>&1; then
  stuck_total="$(sqlite3 ~/.aid/aid.db \
    "SELECT COUNT(*) FROM tasks WHERE status='running' AND created_at < datetime('now','-10 minutes');" 2>/dev/null || echo 0)"
  if (( stuck_total > 0 )); then
    zombie_note="  aid: ${stuck_total} task(s) 'running' > 10min — possible zombies (aid board auto-reaps on v8.85+)"
    bump_sev warn
  fi
fi

# ── aid worktrees for THIS repo ─────────────────────────────────────────────
aid_wt_note=""
aid_wt_count=0
while IFS= read -r git_file; do
  [[ -f "${git_file}" ]] || continue
  parent="$(grep '^gitdir:' "${git_file}" 2>/dev/null | sed -e 's|^gitdir: ||' -e 's|/.git/worktrees/.*||')"
  [[ "${parent}" == "${repo_root}" ]] && ((aid_wt_count+=1))
done < <(find /tmp/aid-wt-* /private/tmp/aid-wt-* -maxdepth 3 -name ".git" -type f 2>/dev/null)
if (( aid_wt_count > 0 )); then
  aid_wt_note="  worktrees: ${aid_wt_count} aid-managed for this repo (run 'aid worktree list' for details)"
  (( aid_wt_count > 3 )) && bump_sev warn
fi

# ── /tmp disk free ──────────────────────────────────────────────────────────
disk_note=""
if df -h /tmp >/dev/null 2>&1; then
  # Extract use% from df line (format varies by platform; parse column 5)
  use_pct="$(df -h /tmp | awk 'NR==2 {gsub("%","",$5); print $5}')"
  if [[ -n "${use_pct}" ]]; then
    disk_note="  disk(/tmp): ${use_pct}% used"
    if (( use_pct > 95 )); then
      disk_note+=" ⚠️  nearly full — clean before dispatching"
      bump_sev crit
    elif (( use_pct > 85 )); then
      disk_note+=" ⚠️  high"
      bump_sev warn
    fi
  fi
fi

# ── emit report ─────────────────────────────────────────────────────────────
header_line
[[ -n "${fetch_note}"   ]] && echo "${fetch_note}"
echo "${aheadbehind}"
[[ -n "${dirty}"        ]] && echo "${dirty}"
[[ -n "${zombie_note}"  ]] && echo "${zombie_note}"
[[ -n "${aid_wt_note}"  ]] && echo "${aid_wt_note}"
[[ -n "${disk_note}"    ]] && echo "${disk_note}"

# Hard failure codes (for hook consumers that care):
# 0 = ok, 1 = warn, 2 = crit
case "${sev}" in
  ok)   exit 0;;
  warn) exit 0;;   # still exit 0 — don't block session start
  crit) exit 0;;
esac
