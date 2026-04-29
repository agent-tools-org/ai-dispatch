#!/usr/bin/env bash
# Applies an aid BYOK manifest to opencode custom-provider config.
# Generates the matching aid custom-agent TOML and verifies the provider model list.
# Dependencies: bash, jq, opencode, grep, date, mktemp, mkdir, cp, mv, cmp, chmod.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/aid-byok-lib.sh"

usage() {
  cat <<'EOF'
Usage: scripts/aid-byok-apply.sh [--dry-run] [--key <api-key>] <manifest.toml>

Environment overrides:
  OPENCODE_CONFIG_DIR  Defaults to ~/.config/opencode
  OPENCODE_AUTH_DIR    Defaults to ~/.local/share/opencode
  AID_HOME             Defaults to ~/.aid
EOF
}

restore_from_backups() {
  local config_backup="$1"
  local auth_backup="$2"
  local agent_backup="$3"
  local agent_path="$4"
  local agent_existed="$5"
  if [[ -n "${config_backup}" ]]; then
    cp "${config_backup}" "$(byok_config_dir)/opencode.json"
  fi
  if [[ -n "${auth_backup}" ]]; then
    cp "${auth_backup}" "$(byok_auth_dir)/auth.json"
    chmod 600 "$(byok_auth_dir)/auth.json"
  fi
  if [[ -n "${agent_backup}" && -f "${agent_backup}" ]]; then
    cp "${agent_backup}" "${agent_path}"
  elif [[ "${agent_existed}" != "true" ]]; then
    rm -f "${agent_path}"
  fi
}

replace_file_if_changed() {
  local path="$1"
  local tmp="$2"
  local ts="$3"
  local mode="${4:-}"
  local backup=""
  if [[ -f "${path}" ]] && cmp -s "${path}" "${tmp}"; then
    rm -f "${tmp}"
    printf '\n'
    return 0
  fi
  if [[ -f "${path}" ]]; then
    backup="$(backup_file "${path}" "${ts}")"
  fi
  mv "${tmp}" "${path}"
  if [[ -n "${mode}" ]]; then
    chmod "${mode}" "${path}"
  fi
  printf '%s\n' "${backup}"
}

print_plan() {
  local manifest_data="$1"
  local key_source="$2"
  local id default_model config_path auth_path agent_path
  id="$(jq -r '.id' <<< "${manifest_data}")"
  default_model="$(jq -r '.default_model' <<< "${manifest_data}")"
  config_path="$(byok_config_dir)/opencode.json"
  auth_path="$(byok_auth_dir)/auth.json"
  agent_path="$(byok_aid_home)/agents/${id}.toml"

  printf 'BYOK apply plan\n'
  printf '  provider: %s\n' "${id}"
  printf '  protocol: openai\n'
  printf '  model: %s/%s\n' "${id}" "${default_model}"
  printf '  key source: %s\n' "${key_source}"
  printf '  opencode config: %s\n' "${config_path}"
  printf '  opencode auth: %s\n' "${auth_path}"
  printf '  aid agent: %s\n' "${agent_path}"
  jq -r '.model[] | "  provider model: \(.id) context=\(.context) output=\(.output)"' <<< "${manifest_data}"
}

parse_args() {
  dry_run="false"
  flag_key=""
  manifest=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --dry-run)
        dry_run="true"
        shift
        ;;
      --key)
        [[ $# -ge 2 ]] || fail "--key requires a value"
        flag_key="$2"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      --*)
        fail "unknown option: $1"
        ;;
      *)
        [[ -z "${manifest}" ]] || fail "only one manifest path is supported"
        manifest="$1"
        shift
        ;;
    esac
  done
  [[ -n "${manifest}" ]] || fail "missing manifest path"
}

apply_manifest() {
  local manifest_data="$1"
  local api_key="$2"
  local id config_path auth_path agent_dir agent_path ts
  local config_backup auth_backup agent_backup provider config_tmp auth_tmp agent_tmp pattern
  local agent_existed="false"
  id="$(jq -r '.id' <<< "${manifest_data}")"
  config_path="$(byok_config_dir)/opencode.json"
  auth_path="$(byok_auth_dir)/auth.json"
  agent_dir="$(byok_aid_home)/agents"
  agent_path="${agent_dir}/${id}.toml"
  ts="$(date +%s)"

  ensure_json_file "${config_path}"
  ensure_json_file "${auth_path}"
  mkdir -p "${agent_dir}"
  if [[ -f "${agent_path}" ]] && ! grep -Fqx "# aid-byok-generated: ${id}" "${agent_path}"; then
    fail "refusing to overwrite non-generated aid agent: ${agent_path}"
  fi
  if [[ -f "${agent_path}" ]]; then
    agent_existed="true"
  fi

  provider="$(provider_block_json "${manifest_data}")"
  config_tmp="$(mktemp "${TMPDIR:-/tmp}/aid-byok-config.XXXXXX")"
  jq --arg id "${id}" --argjson block "${provider}" \
    '.provider = (.provider // {}) | .provider[$id] = $block' \
    "${config_path}" > "${config_tmp}"

  auth_tmp="$(mktemp "${TMPDIR:-/tmp}/aid-byok-auth.XXXXXX")"
  jq --arg id "${id}" --arg key "${api_key}" \
    '.[$id] = {type: "api", key: $key}' \
    "${auth_path}" > "${auth_tmp}"

  agent_tmp="$(mktemp "${TMPDIR:-/tmp}/aid-byok-agent.XXXXXX")"
  write_agent_toml "${manifest_data}" "${agent_tmp}"

  config_backup="$(replace_file_if_changed "${config_path}" "${config_tmp}" "${ts}")"
  auth_backup="$(replace_file_if_changed "${auth_path}" "${auth_tmp}" "${ts}" 600)"
  agent_backup="$(replace_file_if_changed "${agent_path}" "${agent_tmp}" "${ts}")"

  pattern="^${id//./\\.}/"
  if ! opencode models | grep -E "${pattern}" >/dev/null; then
    restore_from_backups "${config_backup}" "${auth_backup}" "${agent_backup}" "${agent_path}" "${agent_existed}"
    fail "opencode did not list models for provider ${id}; restored backups"
  fi

  printf 'BYOK provider applied: %s\n' "${id}"
}

main() {
  require_cmd jq
  parse_args "$@"
  local manifest_data resolved key_source api_key
  manifest_data="$(manifest_json "${manifest}")"
  validate_manifest "${manifest_data}"
  resolved="$(resolve_api_key "${manifest_data}" "${flag_key}" "${dry_run}")"
  key_source="$(key_source_from_resolution "${resolved}")"
  print_plan "${manifest_data}" "${key_source}"
  if [[ "${dry_run}" == "true" ]]; then
    return 0
  fi
  require_cmd opencode
  api_key="$(api_key_from_resolution "${resolved}")"
  apply_manifest "${manifest_data}" "${api_key}"
}

main "$@"
