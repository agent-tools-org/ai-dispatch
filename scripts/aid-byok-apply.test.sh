#!/usr/bin/env bash
# E2E-style smoke test for BYOK apply/remove scripts.
# Creates sandboxed opencode/aid homes and stubs the opencode models command.
# Dependencies: bash, jq, mktemp, mkdir, chmod, grep, stat, ls, wc, rm.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/aid-byok-test.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

fail() {
  echo "test failed: $*" >&2
  exit 1
}

file_mode() {
  local path="$1"
  if stat -f '%Lp' "${path}" >/dev/null 2>&1; then
    stat -f '%Lp' "${path}"
    return 0
  fi
  stat -c '%a' "${path}"
}

write_stub_opencode() {
  local bin_dir="$1"
  mkdir -p "${bin_dir}"
  cat > "${bin_dir}/opencode" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "models" ]]; then
  echo "unsupported opencode stub command: ${1:-}" >&2
  exit 1
fi
config="${OPENCODE_CONFIG_DIR}/opencode.json"
if [[ ! -f "${config}" ]]; then
  exit 0
fi
jq -r '
  (.provider // {})
  | to_entries[]
  | .key as $provider
  | (.value.models // {})
  | keys[]
  | "\($provider)/\(.)"
' "${config}"
STUB
  chmod +x "${bin_dir}/opencode"
}

write_manifest() {
  local manifest="$1"
  cat > "${manifest}" <<'TOML'
[byok]
id = "acme"
display_name = "ACME (token = #abc)"
protocol = "openai"
base_url = "https://api.acme.example/v1"
key_env = "ACME_API_KEY"
default_model = "acme-pro"
timeout_ms = 300000

[[byok.model]]
id = "acme-pro"
name = "ACME Pro"
context = 131072
output = 8192
tool_call = true
reasoning = false

[[byok.model]]
id = "acme-mini"
context = 32768
output = 4096

[byok.capabilities]
research = 5
simple_edit = 6
complex_impl = 5
frontend = 4
debugging = 5
testing = 5
refactoring = 5
documentation = 5
TOML
}

assert_applied() {
  local config_path="$1"
  local auth_path="$2"
  local agent_path="$3"
  jq -e '.provider.acme.npm == "@ai-sdk/openai-compatible"' "${config_path}" >/dev/null
  jq -e '.provider.acme.name == "ACME (token = #abc)"' "${config_path}" >/dev/null
  jq -e '.provider.acme.options.baseURL == "https://api.acme.example/v1"' "${config_path}" >/dev/null
  jq -e '.provider.acme.models["acme-pro"].limit.context == 131072' "${config_path}" >/dev/null
  jq -e '.provider.acme.models["acme-pro"].reasoning == false' "${config_path}" >/dev/null
  jq -e '.provider.acme.models["acme-mini"].tool_call == true' "${config_path}" >/dev/null
  jq -e '.acme == {"type":"api","key":"test-key"}' "${auth_path}" >/dev/null
  grep -Fqx '# aid-byok-generated: acme' "${agent_path}"
  grep -Fq 'opencode run --model acme/acme-pro' "${agent_path}"
  grep -Fq 'simple_edit = 6' "${agent_path}"
}

assert_removed() {
  local config_path="$1"
  local auth_path="$2"
  local agent_path="$3"
  jq -e '(.provider.acme // null) == null' "${config_path}" >/dev/null
  jq -e '(.acme // null) == null' "${auth_path}" >/dev/null
  [[ ! -e "${agent_path}" ]] || fail "generated agent still exists"
}

main() {
  local bin_dir="${tmp_dir}/bin"
  local manifest="${tmp_dir}/acme.toml"
  local config_dir="${tmp_dir}/config"
  local auth_dir="${tmp_dir}/auth"
  local aid_home="${tmp_dir}/aid"
  local config_path="${config_dir}/opencode.json"
  local auth_path="${auth_dir}/auth.json"
  local agent_path="${aid_home}/agents/acme.toml"
  local auth_backup backup_count backup_mode

  write_stub_opencode "${bin_dir}"
  write_manifest "${manifest}"
  mkdir -p "${config_dir}" "${auth_dir}"
  printf '{"provider":{"opencode-go":{"name":"keep-me","models":{}}}}\n' > "${config_path}"
  printf '{}\n' > "${auth_path}"
  chmod 644 "${auth_path}"

  export PATH="${bin_dir}:${PATH}"
  export OPENCODE_CONFIG_DIR="${config_dir}"
  export OPENCODE_AUTH_DIR="${auth_dir}"
  export AID_HOME="${aid_home}"
  export ACME_API_KEY="test-key"

  bash "${repo_root}/scripts/aid-byok-apply.sh" "${manifest}" >/dev/null
  assert_applied "${config_path}" "${auth_path}" "${agent_path}"
  jq -e '.provider["opencode-go"].name == "keep-me"' "${config_path}" >/dev/null
  auth_backup="$(ls "${auth_path}".bak.*)"
  backup_mode="$(file_mode "${auth_backup}")"
  [[ "${backup_mode}" == "600" ]] || fail "auth backup mode was ${backup_mode}, expected 600"

  bash "${repo_root}/scripts/aid-byok-apply.sh" "${manifest}" >/dev/null
  assert_applied "${config_path}" "${auth_path}" "${agent_path}"
  backup_count="$(ls "${auth_path}".bak.* "${config_path}".bak.* | wc -l | tr -d ' ')"
  [[ "${backup_count}" == "2" ]] || fail "expected one backup set, found ${backup_count} files"

  bash "${repo_root}/scripts/aid-byok-remove.sh" "${manifest}" >/dev/null
  assert_removed "${config_path}" "${auth_path}" "${agent_path}"
  jq -e '.provider["opencode-go"].name == "keep-me"' "${config_path}" >/dev/null
}

main "$@"
