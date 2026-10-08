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

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/../src/remote_build/cargo.sh"
remote_checkout_layout
remote_cmd="export CARGO_TARGET_DIR=${remote_target}; exec cargo test --workspace \"\$@\""
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
if [[ $status -eq 0 ]] && awk '
  /^test result: / && match($0, /[0-9]+ passed; [0-9]+ failed; [0-9]+ ignored;/) {
    split(substr($0, RSTART, RLENGTH), counts, /[^0-9]+/)
    total += counts[1] + counts[2] + counts[3]
    found = 1
  }
  END { exit !(found && total == 0) }
' "$log"; then
  echo "[remote-test] no tests matched the filter" >&2
  exit 66
fi
job="$(awk '/^rbox: job / { print $3; exit }' "$log")"
job="${job:-not started (no job id assigned)}"
if grep -q '^rbox: job .* exited with code ' "$log" && [[ $status -ne 0 ]]; then
  echo "[remote-test] job ${job}: tests failed (exit ${status})" >&2
else
  case "$status" in
    124) echo "[remote-test] job ${job}: timed out after ${TIMEOUT}s; the job continues on the box" >&2 ;;
    75) echo "[remote-test] job ${job}: box lock not acquired within ${LOCK_TIMEOUT}s" >&2 ;;
    69) echo "[remote-test] job ${job}: box ${BOX} refused admission (disk); no test ran" >&2 ;;
    0) ;;
    *) echo "[remote-test] rbox failed before any test ran (exit ${status}); see the rbox line above" >&2 ;;
  esac
fi
exit "$status"
