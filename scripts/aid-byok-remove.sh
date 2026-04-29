#!/usr/bin/env bash
# Removes a BYOK provider from opencode config and auth state.
# Deletes the matching aid agent only when it has the BYOK generated marker.
# Dependencies: bash, jq, opencode, grep, date, mktemp, mkdir, cp, mv, chmod, rm.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/aid-byok-lib.sh"

usage() {
  cat <<'EOF'
Usage: scripts/aid-byok-remove.sh <manifest.toml|provider-id>

Environment overrides:
  OPENCODE_CONFIG_DIR  Defaults to ~/.config/opencode
  OPENCODE_AUTH_DIR    Defaults to ~/.local/share/opencode
  AID_HOME             Defaults to ~/.aid
EOF
}

provider_id_from_arg() {
  local input="$1"
  local manifest_data id
  if [[ -f "${input}" ]]; then
    manifest_data="$(manifest_json "${input}")"
    id="$(jq -r '.id // ""' <<< "${manifest_data}")"
  else
    id="${input}"
  fi
  [[ "${id}" =~ ^[A-Za-z0-9._-]+$ ]] || fail "invalid provider id: ${id}"
  printf '%s\n' "${id}"
}

remove_generated_agent() {
  local id="$1"
  local agent_path
  agent_path="$(byok_aid_home)/agents/${id}.toml"
  [[ -f "${agent_path}" ]] || return 0
  if grep -Fqx "# aid-byok-generated: ${id}" "${agent_path}"; then
    rm -f "${agent_path}"
    return 0
  fi
  printf 'Skipping non-generated aid agent: %s\n' "${agent_path}" >&2
}

remove_provider() {
  local id="$1"
  local config_path auth_path agent_path ts config_backup auth_backup agent_backup tmp pattern
  config_path="$(byok_config_dir)/opencode.json"
  auth_path="$(byok_auth_dir)/auth.json"
  agent_path="$(byok_aid_home)/agents/${id}.toml"
  ts="$(date +%s)"

  ensure_json_file "${config_path}"
  ensure_json_file "${auth_path}"
  config_backup="$(backup_file "${config_path}" "${ts}")"
  auth_backup="$(backup_file "${auth_path}" "${ts}")"
  agent_backup=""
  if [[ -f "${agent_path}" ]]; then
    agent_backup="$(backup_file "${agent_path}" "${ts}")"
  fi

  tmp="$(mktemp "${TMPDIR:-/tmp}/aid-byok-config.XXXXXX")"
  jq --arg id "${id}" 'if .provider then del(.provider[$id]) else . end' \
    "${config_path}" > "${tmp}"
  mv "${tmp}" "${config_path}"

  tmp="$(mktemp "${TMPDIR:-/tmp}/aid-byok-auth.XXXXXX")"
  jq --arg id "${id}" 'del(.[$id])' "${auth_path}" > "${tmp}"
  mv "${tmp}" "${auth_path}"
  chmod 600 "${auth_path}"
  remove_generated_agent "${id}"

  pattern="^${id//./\\.}/"
  if opencode models | grep -E "${pattern}" >/dev/null; then
    cp "${config_backup}" "${config_path}"
    cp "${auth_backup}" "${auth_path}"
    chmod 600 "${auth_path}"
    if [[ -n "${agent_backup}" && -f "${agent_backup}" ]]; then
      cp "${agent_backup}" "${agent_path}"
    fi
    fail "opencode still lists provider ${id}; restored config, auth, and agent backups"
  fi

  printf 'BYOK provider removed: %s\n' "${id}"
}

main() {
  require_cmd jq
  require_cmd opencode
  if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
    usage
    [[ $# -eq 1 ]] && exit 0
    exit 1
  fi
  local id
  id="$(provider_id_from_arg "$1")"
  remove_provider "${id}"
}

main "$@"
