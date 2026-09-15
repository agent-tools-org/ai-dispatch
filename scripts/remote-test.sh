#!/usr/bin/env bash
# Run workspace tests through one rbox exec; exit with its status.
# Usage: scripts/remote-test.sh [--dry-run] [--jobs N] [--timeout S] [--lock-timeout S] [-- <cargo args>]
# Dependencies: bash, git, rbox, tee, awk, grep; AID_BUILD_BOX selects the configured box.
set -euo pipefail

BOX="${AID_BUILD_BOX:-}"
JOBS="${AID_BUILD_JOBS:-4}"
TIMEOUT=5400
LOCK_TIMEOUT=3600
DRY_RUN=false
EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs|--timeout|--lock-timeout)
      [[ $# -ge 2 ]] || { echo "missing value for $1" >&2; exit 2; }
      case "$1" in
        --jobs) JOBS="$2" ;;
        --timeout) TIMEOUT="$2" ;;
        --lock-timeout) LOCK_TIMEOUT="$2" ;;
      esac
      shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    --) shift; EXTRA=("$@"); break ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -n "$BOX" ]] || { echo "AID_BUILD_BOX is not set; export the configured rbox build box name" >&2; exit 2; }

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
git rev-parse --verify HEAD >/dev/null
common_dir="$(cd "$(git rev-parse --git-common-dir)" && pwd)"
repo_name="$(basename "$(dirname "$common_dir")")"
repo_name="$(printf '%s' "$repo_name" | LC_ALL=C tr -c 'a-zA-Z0-9_-' '-')"
checkout_id="$(git symbolic-ref --quiet --short HEAD || git rev-parse --short HEAD)"
checkout_id="$(printf '%s' "$checkout_id" | LC_ALL=C tr -c 'a-zA-Z0-9_-' '-')"
remote_dir="~/.rbox/work/${repo_name}/${checkout_id}"
remote_cmd="export CARGO_TARGET_DIR=\$HOME/.rbox/target/${repo_name}; exec cargo test --workspace \"\$@\""
command=(rbox exec "$BOX" "$repo_root" --to "$remote_dir" --jobs "$JOBS"
  --timeout "$TIMEOUT" --lock-timeout "$LOCK_TIMEOUT" -- bash -c "$remote_cmd"
  remote-test ${EXTRA[@]+"${EXTRA[@]}"})

if "$DRY_RUN"; then
  for arg in "${command[@]}"; do
    quoted="$(printf '%q' "$arg")"
    [[ $quoted != '~'* ]] || printf '\\'
    printf '%s ' "$quoted"
  done
  printf '\n'
  exit 0
fi
command -v rbox >/dev/null || { echo "rbox CLI not found on PATH" >&2; exit 2; }

log="$(mktemp)"
trap 'rm -f "$log"' EXIT
status=0
"${command[@]}" 2>&1 | tee "$log" || status=${PIPESTATUS[0]}
job="$(awk '/^rbox: job / { print $3; exit }' "$log")"
job="${job:-not started (no job id assigned)}"
if grep -q '^rbox: job .* exited with code ' "$log" && [[ $status -ne 0 ]]; then
  echo "[remote-test] job ${job}: tests failed (exit ${status})" >&2
else
  case "$status" in
    124) echo "[remote-test] job ${job}: timed out after ${TIMEOUT}s; the job continues on the box" >&2 ;;
    75) echo "[remote-test] job ${job}: box lock not acquired within ${LOCK_TIMEOUT}s" >&2 ;;
    69) echo "[remote-test] job ${job}: box ${BOX} refused admission (disk); no test ran" >&2 ;;
  esac
fi
exit "$status"
