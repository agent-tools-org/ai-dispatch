#!/usr/bin/env bash
# Shared helpers for BYOK provider scripts.
# Exports manifest parsing, path defaults, JSON builders, and agent TOML generation.
# Dependencies: bash, python3, jq, date, mkdir, cp, chmod.

fail() {
  echo "byok failed: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

byok_config_dir() {
  printf '%s\n' "${OPENCODE_CONFIG_DIR:-${HOME}/.config/opencode}"
}

byok_auth_dir() {
  printf '%s\n' "${OPENCODE_AUTH_DIR:-${HOME}/.local/share/opencode}"
}

byok_aid_home() {
  printf '%s\n' "${AID_HOME:-${HOME}/.aid}"
}

toml_quote() {
  jq -Rn --arg value "$1" '$value'
}

manifest_json() {
  local manifest="$1"
  [[ -f "${manifest}" ]] || fail "manifest not found: ${manifest}"
  command -v python3 >/dev/null 2>&1 || fail "python3 not found; required for TOML parsing"
  python3 - "${manifest}" <<'PY'
import json
import sys
import tomllib

path = sys.argv[1]
try:
    with open(path, "rb") as manifest_file:
        data = tomllib.load(manifest_file)
except tomllib.TOMLDecodeError as exc:
    print(f"byok failed: invalid manifest TOML: {exc}", file=sys.stderr)
    sys.exit(1)
except OSError as exc:
    print(f"byok failed: could not read manifest: {exc}", file=sys.stderr)
    sys.exit(1)

byok = data.get("byok", {})
if not isinstance(byok, dict):
    byok = {}
json.dump(byok, sys.stdout, separators=(",", ":"))
print()
PY
}

validate_manifest() {
  local manifest_data="$1"
  jq -e '
    (.id | type == "string" and length > 0) and
    (.protocol == "openai") and
    (.base_url | type == "string" and length > 0) and
    (.default_model | type == "string" and length > 0) and
    ((.model // []) | length > 0) and
    all(.model[]; (.id | type == "string" and length > 0)
      and (.context | type == "number")
      and (.output | type == "number"))
  ' <<< "${manifest_data}" >/dev/null \
    || fail "manifest must define byok id, protocol=openai, base_url, default_model, and model id/context/output"

  local id
  id="$(jq -r '.id' <<< "${manifest_data}")"
  [[ "${id}" =~ ^[A-Za-z0-9._-]+$ ]] || fail "provider id contains unsupported characters: ${id}"
}

resolve_api_key() {
  local manifest_data="$1"
  local flag_key="$2"
  local dry_run="$3"
  local api_key key_env
  api_key="$(jq -r '.api_key // ""' <<< "${manifest_data}")"
  key_env="$(jq -r '.key_env // ""' <<< "${manifest_data}")"

  if [[ -n "${flag_key}" ]]; then
    printf '%s\n' "flag:${flag_key}"
    return 0
  fi
  if [[ -n "${api_key}" ]]; then
    printf '%s\n' "manifest:${api_key}"
    return 0
  fi
  if [[ -n "${key_env}" ]]; then
    if [[ "${dry_run}" == "true" && -z "${!key_env:-}" ]]; then
      printf '%s\n' "env:${key_env}:"
      return 0
    fi
    [[ -n "${!key_env:-}" ]] || fail "environment variable ${key_env} is not set"
    printf '%s\n' "env:${key_env}:${!key_env}"
    return 0
  fi
  fail "no API key found; use --key, api_key, or key_env"
}

api_key_from_resolution() {
  local resolved="$1"
  case "${resolved}" in
    flag:*|manifest:*) printf '%s\n' "${resolved#*:}" ;;
    env:*:*) printf '%s\n' "${resolved#*:*:}" ;;
    *) fail "invalid key resolution" ;;
  esac
}

key_source_from_resolution() {
  local resolved="$1"
  case "${resolved}" in
    flag:*) printf '%s\n' "--key flag" ;;
    manifest:*) printf '%s\n' "manifest api_key" ;;
    env:*:*)
      local without_prefix="${resolved#env:}"
      printf '%s\n' "env ${without_prefix%%:*}"
      ;;
    *) fail "invalid key resolution" ;;
  esac
}

provider_block_json() {
  local manifest_data="$1"
  jq '
    {
      npm: "@ai-sdk/openai-compatible",
      name: (.display_name // .id),
      options: (
        {baseURL: .base_url}
        + (if has("timeout_ms") then {timeout: .timeout_ms} else {} end)
      ),
      models: (
        reduce .model[] as $model ({};
          .[$model.id] = {
            name: ($model.name // $model.id),
            tool_call: (if $model.tool_call == null then true else $model.tool_call end),
            reasoning: (if $model.reasoning == null then false else $model.reasoning end),
            limit: {context: $model.context, output: $model.output}
          }
        )
      )
    }
  ' <<< "${manifest_data}"
}

ensure_json_file() {
  local path="$1"
  mkdir -p "$(dirname "${path}")"
  if [[ ! -f "${path}" ]]; then
    printf '{}\n' > "${path}"
  fi
  jq -e . "${path}" >/dev/null || fail "invalid JSON file: ${path}"
}

backup_file() {
  local path="$1"
  local ts="$2"
  local backup="${path}.bak.${ts}"
  cp "${path}" "${backup}"
  chmod 600 "${backup}"
  printf '%s\n' "${backup}"
}

write_agent_toml() {
  local manifest_data="$1"
  local agent_path="$2"
  local id default_model display_name protocol model_ref wrapper_args
  id="$(jq -r '.id' <<< "${manifest_data}")"
  default_model="$(jq -r '.default_model' <<< "${manifest_data}")"
  display_name="$(jq -r '.display_name // .id' <<< "${manifest_data}")"
  protocol="$(jq -r '.protocol // "openai"' <<< "${manifest_data}")"
  model_ref="${id}/${default_model}"
  wrapper_args="$(jq -cn \
    --arg script "exec opencode run --format json --model ${model_ref} \"\$@\"" \
    --arg label "aid-byok-${id}" \
    '["-lc", $script, $label]')"

  {
    printf '# aid-byok-generated: %s\n' "${id}"
    printf '# Auto-generated by scripts/aid-byok-apply.sh; remove with scripts/aid-byok-remove.sh.\n'
    if [[ "${protocol}" == "openai" ]]; then
      printf '# delegate_to=opencode: aid resolves this agent through the OpenCode adapter\n'
      printf '#   so opencode read-only/result-file/JSONL streaming work natively.\n'
      printf '#   The fixed_args bash wrapper below is kept as a fallback only.\n'
    else
      printf '# streaming + jsonl required so TUI/show can render structured events.\n'
    fi
    printf '[agent]\n'
    printf 'id = %s\n' "$(toml_quote "${id}")"
    printf 'display_name = %s\n' "$(toml_quote "${display_name}")"
    printf 'command = "bash"\n'
    printf 'prompt_mode = "arg"\n'
    printf 'fixed_args = %s\n' "${wrapper_args}"
    printf 'streaming = true\n'
    printf 'output_format = "jsonl"\n'
    printf 'trust_tier = "api"\n'
    if [[ "${protocol}" == "openai" ]]; then
      printf 'delegate_to = "opencode"\n'
      printf 'forced_model = %s\n' "$(toml_quote "${model_ref}")"
    fi
    jq -r '
      .capabilities // empty
      | to_entries
      | if length > 0 then
          "\n[agent.capabilities]\n" + (map("\(.key) = \(.value)") | join("\n"))
        else empty end
    ' <<< "${manifest_data}"
  } > "${agent_path}"
}
