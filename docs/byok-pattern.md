<!--
Purpose: Documents the BYOK custom-provider pattern for aid users.
Exports: Manifest schema, apply/probe/remove workflows, and an ACME walkthrough.
Dependencies: opencode custom providers, jq/python helper scripts in scripts/.
-->

# BYOK Provider Pattern

The BYOK pattern lets `aid` route an `opencode` custom provider as a normal aid custom agent without modifying `opencode-go`, built-in providers, or Rust agent code. Each provider is described by a TOML manifest, then applied into:

- `~/.config/opencode/opencode.json` under `provider.<id>`
- `~/.local/share/opencode/auth.json` under `<id>`
- `~/.aid/agents/<id>.toml` as an aid custom agent

MiMo is the canonical real example. See `examples/byok/mimo.toml`; it uses `key_env = "MIMO_API_KEY"` and does not store the real API key in the repo.

## Requirements

The helper scripts require `bash`, `jq`, `python3` with stdlib `tomllib`, and `opencode` for non-dry-run apply/remove/probe flows.

## Manifest Schema

```toml
# ~/.aid/byok/<id>.toml
[byok]
id = "acme"                                 # required, used as opencode provider key + aid agent id
display_name = "ACME (corporate plan)"      # optional
protocol = "openai"                         # required, MVP only supports "openai"
base_url = "https://api.acme.example/v1"    # required
key_env = "ACME_API_KEY"                    # optional, env var name
api_key = "..."                             # optional, literal (discouraged; prefer key_env)
default_model = "acme-pro"                  # required, used by generated aid agent
timeout_ms = 300000                         # optional

[[byok.model]]
id = "acme-pro"                             # required
name = "ACME Pro"                           # optional
context = 131072                            # required
output = 8192                               # required
tool_call = true                            # default true
reasoning = false                           # default false

[[byok.model]]
id = "acme-mini"
context = 32768
output = 4096

[byok.capabilities]                         # optional, copied into generated aid agent TOML
research = 5
simple_edit = 6
complex_impl = 5
frontend = 4
debugging = 5
testing = 5
refactoring = 5
documentation = 5
```

Required fields are `id`, `protocol = "openai"`, `base_url`, `default_model`, and at least one `[[byok.model]]` with `id`, `context`, and `output`.

API keys resolve in this order:

1. `--key <api-key>` passed to `scripts/aid-byok-apply.sh` or `scripts/aid-byok-probe.sh`
2. `[byok].api_key`
3. Environment variable named by `[byok].key_env`

Prefer `key_env` for checked-in manifests.

## Apply Flow

Run:

```bash
export ACME_API_KEY="..."
bash scripts/aid-byok-apply.sh ~/.aid/byok/acme.toml
```

For inspection only:

```bash
bash scripts/aid-byok-apply.sh --dry-run examples/byok/mimo.toml
```

`apply` validates the manifest, resolves the API key, backs up both opencode JSON files with timestamped `.bak.<unix-ts>` files, and merges only the target provider:

```jq
.provider = (.provider // {}) | .provider[$id] = $block
```

That means existing siblings such as `provider.opencode`, `provider.opencode-go`, and unrelated custom providers are preserved. The script never replaces the whole `provider` object.

The auth entry is added or replaced as:

```json
{"<id>": {"type": "api", "key": "..."}}
```

The auth file is written with mode `600`.

After writing, `apply` runs `opencode models` and expects a model line beginning with `<id>/`. If verification fails, it restores the opencode config and auth backups and exits non-zero.

All paths are overrideable for tests and sandboxes:

```bash
OPENCODE_CONFIG_DIR=/tmp/opencode-config \
OPENCODE_AUTH_DIR=/tmp/opencode-auth \
AID_HOME=/tmp/aid \
bash scripts/aid-byok-apply.sh ~/.aid/byok/acme.toml
```

## Generated Aid Agent

`apply` writes `~/.aid/agents/<id>.toml` with a marker comment:

```toml
# aid-byok-generated: acme
[agent]
id = "acme"
display_name = "ACME (corporate plan)"
command = "bash"
prompt_mode = "arg"
fixed_args = ["-lc", "exec opencode run --model acme/acme-pro \"$@\"", "aid-byok-acme"]
trust_tier = "api"
```

When aid dispatches this custom agent, the user prompt is passed through the bash wrapper to:

```bash
opencode run --model acme/acme-pro "<prompt>"
```

The `<id>/<default_model>` model id is what keeps routing explicit per call.

If `[byok.capabilities]` is present, those scores are copied to `[agent.capabilities]` so the generated agent can participate in aid selection using the existing custom-agent surface.

If `~/.aid/agents/<id>.toml` already exists without the generated marker, `apply` refuses to overwrite it.

## Add A New Provider

1. Create a manifest:

```bash
mkdir -p ~/.aid/byok
cat > ~/.aid/byok/acme.toml <<'EOF'
[byok]
id = "acme"
display_name = "ACME (corporate plan)"
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
EOF
```

2. Export the key and inspect the planned changes:

```bash
export ACME_API_KEY="sk-..."
bash scripts/aid-byok-apply.sh --dry-run ~/.aid/byok/acme.toml
```

3. Apply the provider:

```bash
bash scripts/aid-byok-apply.sh ~/.aid/byok/acme.toml
```

4. Probe tool-call support:

```bash
bash scripts/aid-byok-probe.sh ~/.aid/byok/acme.toml
```

The probe calls `/chat/completions` with a dummy `get_weather(city)` tool and reports `tool_calls: yes/no`, `finish_reason`, and a one-line content preview. It exits `0` when tool calls are present and `2` otherwise.

5. Use the generated aid agent:

```bash
aid run acme "Summarize the retry flow in this repo" --dir .
```

## Coexistence

BYOK providers coexist with built-in opencode subscriptions and other custom providers. Authentication stays isolated by provider id in `auth.json`; config stays isolated under `provider.<id>` in `opencode.json`.

Routing is per call by model id. `opencode run --model acme/acme-pro` uses ACME, while `opencode run --model opencode/...` or `opencode-go/...` continues using those existing providers.

## Remove Flow

Remove by manifest path:

```bash
bash scripts/aid-byok-remove.sh ~/.aid/byok/acme.toml
```

Or remove by provider id:

```bash
bash scripts/aid-byok-remove.sh acme
```

`remove` backs up opencode config and auth files, deletes only `provider.<id>`, removes only the auth entry named `<id>`, and deletes `~/.aid/agents/<id>.toml` only when the file contains the `# aid-byok-generated: <id>` marker. Hand-written agent files are left in place.

After deletion, `remove` runs `opencode models` and fails if any line still begins with `<id>/`.
