#!/usr/bin/env bash
# Run the workspace test suite on a remote build box over Tailscale SSH.
# Usage: AID_BUILD_BOX=<tailscale-hostname> scripts/remote-test.sh [--jobs N] [-- <extra cargo test args>]
# Ships tracked files only (git ls-files); the box keeps /root/build/<repo> so cargo's
# target cache stays warm between runs. Exit status is cargo's.
set -euo pipefail

BOX="${AID_BUILD_BOX:-}"
USER_="${AID_BUILD_USER:-root}"
JOBS="${AID_BUILD_JOBS:-4}"
EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs) JOBS="$2"; shift 2 ;;
    --) shift; EXTRA=("$@"); break ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -n "$BOX" ]] || { echo "AID_BUILD_BOX is not set (Tailscale hostname of the build box)" >&2; exit 2; }
command -v tailscale >/dev/null || { echo "tailscale CLI not found" >&2; exit 2; }

repo_root="$(git rev-parse --show-toplevel)"
repo_name="$(basename "$repo_root")"
remote_dir="/root/build/${repo_name}"
target="${USER_}@${BOX}"
cd "$repo_root"

echo "[remote-test] syncing $(git ls-files | wc -l | tr -d ' ') tracked files to ${target}:${remote_dir}" >&2
# Replace everything except target/ so deleted files disappear too.
git ls-files -z | COPYFILE_DISABLE=1 tar --null -T - --no-xattrs -czf - \
  | tailscale ssh "$target" "mkdir -p '${remote_dir}' && cd '${remote_dir}' \
      && find . -mindepth 1 -maxdepth 1 ! -name target -exec rm -rf {} + && tar -xzf -"

echo "[remote-test] cargo test --workspace -j${JOBS} ${EXTRA[*]:-}" >&2
# Clean, small environment: the box's login shell is not ours, and a rustup
# toolchain under the login home must win over any distro cargo.
tailscale ssh "$target" "cd '${remote_dir}' && env -i HOME=\$HOME USER=${USER_} TERM=dumb LANG=C.UTF-8 \
  PATH=\$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
  CARGO_BUILD_JOBS=${JOBS} CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0 \
  cargo test --workspace ${EXTRA[*]:-}"
