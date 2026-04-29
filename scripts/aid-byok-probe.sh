#!/usr/bin/env bash
# Probes a BYOK OpenAI-compatible endpoint for tool-call behavior.
# Sends a chat completion with one dummy tool and reports the provider response shape.
# Dependencies: bash, jq, curl, mktemp, rm.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/aid-byok-lib.sh"

usage() {
  cat <<'EOF'
Usage: scripts/aid-byok-probe.sh [--key <api-key>] <manifest.toml>

Exit codes:
  0  tool_calls present
  2  no tool_calls present
EOF
}

parse_args() {
  flag_key=""
  manifest=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
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

request_json() {
  local model="$1"
  local prompt="What's the weather in Tokyo? Use the tool."
  jq -cn --arg model "${model}" --arg prompt "${prompt}" '{
    model: $model,
    messages: [
      {role: "user", content: $prompt}
    ],
    tools: [{
      type: "function",
      function: {
        name: "get_weather",
        description: "Return current weather for a city.",
        parameters: {
          type: "object",
          properties: {city: {type: "string"}},
          required: ["city"]
        }
      }
    }],
    tool_choice: "auto",
    max_tokens: 128
  }'
}

probe_manifest() {
  local manifest_data="$1"
  local api_key="$2"
  local base_url model url body_file status has_tool_calls finish_reason preview
  base_url="$(jq -r '.base_url' <<< "${manifest_data}")"
  model="$(jq -r '.default_model' <<< "${manifest_data}")"
  url="${base_url%/}/chat/completions"
  body_file="$(mktemp "${TMPDIR:-/tmp}/aid-byok-probe.XXXXXX")"

  status="$(curl -sS -o "${body_file}" -w '%{http_code}' \
    -H "Authorization: Bearer ${api_key}" \
    -H "Content-Type: application/json" \
    -d "$(request_json "${model}")" \
    "${url}")" || {
      rm -f "${body_file}"
      fail "curl request failed"
    }

  if [[ ! "${status}" =~ ^2 ]]; then
    printf 'HTTP status: %s\n' "${status}" >&2
    jq -r '.error.message // .message // .' "${body_file}" >&2 || true
    rm -f "${body_file}"
    exit 1
  fi

  has_tool_calls="$(jq -r '((.choices[0].message.tool_calls // []) | length) > 0' "${body_file}")"
  finish_reason="$(jq -r '.choices[0].finish_reason // "unknown"' "${body_file}")"
  preview="$(jq -r '.choices[0].message.content // ""' "${body_file}")"
  preview="${preview//$'\n'/ }"
  preview="${preview:0:120}"
  rm -f "${body_file}"

  if [[ "${has_tool_calls}" == "true" ]]; then
    printf 'tool_calls: yes\n'
    printf 'finish_reason: %s\n' "${finish_reason}"
    printf 'content: %s\n' "${preview}"
    return 0
  fi
  printf 'tool_calls: no\n'
  printf 'finish_reason: %s\n' "${finish_reason}"
  printf 'content: %s\n' "${preview}"
  return 2
}

main() {
  require_cmd jq
  require_cmd curl
  parse_args "$@"
  local manifest_data resolved api_key
  manifest_data="$(manifest_json "${manifest}")"
  validate_manifest "${manifest_data}"
  resolved="$(resolve_api_key "${manifest_data}" "${flag_key}" "false")"
  api_key="$(api_key_from_resolution "${resolved}")"
  probe_manifest "${manifest_data}" "${api_key}"
}

main "$@"
