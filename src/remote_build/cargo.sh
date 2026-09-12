#!/usr/bin/env bash
# Cargo PATH shim: build commands use rbox; other commands use host Cargo.
# Dependencies: Bash, git, rbox; AID_BUILD_BOX is resolved by task dispatch.
set -uo pipefail
shim_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
clean_path=()
IFS=: read -r -a path_parts <<< "$PATH:"
for part in "${path_parts[@]}"; do
  [[ "$part" == "$shim_dir" ]] || clean_path+=("$part")
done
export PATH="$(IFS=:; echo "${clean_path[*]}")"
subcommand=""
skip_value=false
for arg in "$@"; do
  if "$skip_value"; then skip_value=false; continue; fi
  case "$arg" in
    --version|-V|--list) exec cargo "$@" ;;
    --color|--config|-C|-Z) skip_value=true ;;
    +*|--*=*|--verbose|-v|-vv|--quiet|-q|--locked|--offline|--frozen|-Z*|-C*) ;;
    --help|-h) exec cargo "$@" ;;
    -*) ;;
    *) subcommand="$arg"; break ;;
  esac
done
case "$subcommand" in
  build|check|test|clippy|bench|doc) ;;
  *) exec cargo "$@" ;;
esac
set -e
repo_root="$(git rev-parse --show-toplevel)"
relative_dir="$(git rev-parse --show-prefix)"
printf -v remote_cwd '%q' "./$relative_dir"
cd "$repo_root"
git rev-parse --verify HEAD >/dev/null
common_dir="$(cd "$(git rev-parse --git-common-dir)" && pwd)"
repo_name="$(basename "$(dirname "$common_dir")")"
repo_name="$(printf '%s' "$repo_name" | LC_ALL=C tr -c 'a-zA-Z0-9_-' '-')"
checkout_id="$(git symbolic-ref --quiet --short HEAD || git rev-parse --short HEAD)"
checkout_id="$(printf '%s' "$checkout_id" | LC_ALL=C tr -c 'a-zA-Z0-9_-' '-')"
remote_cmd="mkdir -p -- ${remote_cwd} && cd -- ${remote_cwd} && export CARGO_TARGET_DIR=\$HOME/.rbox/target/${repo_name} && exec cargo \"\$@\""
unset CARGO_TARGET_DIR
rbox exec "$AID_BUILD_BOX" "$repo_root" --to "~/.rbox/work/${repo_name}/${checkout_id}" \
  --untracked --jobs "${AID_BUILD_JOBS:-4}" --timeout 3600 --lock-timeout 900 \
  -- bash -c "$remote_cmd" remote-cargo "$@" &
job_pid=$!
heartbeat() {
  elapsed=0
  trap 'kill "${sleep_pid:-}" 2>/dev/null || :; exit 0' TERM INT
  while true; do
    sleep 60 & sleep_pid=$!
    wait "$sleep_pid" || return
    kill -0 "$job_pid" 2>/dev/null || return
    elapsed=$((elapsed + 60))
    echo "[remote-build] $AID_BUILD_BOX: still running (${elapsed}s)" >&2
  done
}
heartbeat &
heartbeat_pid=$!
trap 'kill "$heartbeat_pid" 2>/dev/null || :; wait "$heartbeat_pid" 2>/dev/null || :' EXIT
status=0
wait "$job_pid" || status=$?
case "$status" in
  75) echo "[remote-build] $AID_BUILD_BOX: box lock not acquired" >&2 ;;
  124) echo "[remote-build] $AID_BUILD_BOX: job still running on the box" >&2 ;;
esac
exit "$status"
