#!/usr/bin/env bash
# Live acceptance probe for the aid client HTTP API (docs/design/client-api.md).
# Starts a real `aid web` against a COPY of the store and asserts observable behaviour.
# Exits non-zero if any check fails. A check whose precondition is absent prints SKIP
# loudly and fails the run — a probe that quietly covers nothing is worse than no probe.
set -uo pipefail

BIN="${AID_BIN:-}"
if [[ -z "$BIN" ]]; then
  echo "set AID_BIN to the aid binary built with --features web" >&2
  exit 2
fi

PORT="${PROBE_PORT:-8971}"
TOKEN="probe-token-$$"
HOME_DIR="$(mktemp -d)"
SRC_DB="${AID_SRC_DB:-$HOME/.aid/aid.db}"
PASS=0 FAIL=0 SKIP=0

pass() { echo "  PASS  $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL  $1"; FAIL=$((FAIL + 1)); }
skip() { echo "  SKIP  $1"; SKIP=$((SKIP + 1)); }

cleanup() { pkill -f "web --host 127.0.0.1 --port $PORT" 2>/dev/null; pkill -f "web --host 0.0.0.0 --port $PORT" 2>/dev/null; rm -rf "$HOME_DIR"; }
trap cleanup EXIT

# Work on a copy so probes never write to the operator's live tasks.
if [[ -f "$SRC_DB" ]]; then cp "$SRC_DB" "$HOME_DIR/aid.db"; fi

start_loopback() {
  ( AID_HOME="$HOME_DIR" "$BIN" web --host 127.0.0.1 --port "$PORT" --token "$TOKEN" >"$HOME_DIR/server.log" 2>&1 & )
  for _ in $(seq 1 20); do
    sleep 1
    curl -s -o /dev/null --max-time 5 "http://127.0.0.1:$PORT/api/usage" -H "Authorization: Bearer $TOKEN" && return 0
  done
  return 1
}

code() { curl -s -o /dev/null -w '%{http_code}' --max-time 30 "$@"; }

echo "== auth =="
if ! start_loopback; then echo "server did not come up; see $HOME_DIR/server.log"; exit 1; fi
[[ "$(code "http://127.0.0.1:$PORT/api/fleet" -H "Authorization: Bearer $TOKEN")" == 200 ]] \
  && pass "correct bearer is 200" || fail "correct bearer is 200"
[[ "$(code "http://127.0.0.1:$PORT/api/fleet" -H "Authorization: Bearer wrong")" == 401 ]] \
  && pass "wrong bearer is 401" || fail "wrong bearer is 401"
[[ "$(code "http://127.0.0.1:$PORT/api/fleet")" == 401 ]] \
  && pass "missing bearer is 401" || fail "missing bearer is 401"

echo "== query token is for the event stream only =="
[[ "$(code "http://127.0.0.1:$PORT/api/events?token=$TOKEN" --max-time 3)" == 200 ]] \
  && pass "SSE accepts ?token=" || fail "SSE accepts ?token="

echo "== illegal actions are 409, never 5xx =="
RUNNING=$(curl -s --max-time 30 "http://127.0.0.1:$PORT/api/tasks" -H "Authorization: Bearer $TOKEN" \
  | python3 -c "import sys,json;d=json.load(sys.stdin);r=[t['id'] for t in d if t.get('status')=='running'];print(r[0] if r else '')" 2>/dev/null)
if [[ -z "$RUNNING" ]]; then
  skip "no running task in the store copy — merge/steer conflict mapping unverified"
else
  MERGE=$(code -X POST "http://127.0.0.1:$PORT/api/tasks/$RUNNING/merge" -H "Authorization: Bearer $TOKEN")
  [[ "$MERGE" == 409 ]] && pass "merge on a running task is 409" || fail "merge on a running task is 409 (got $MERGE)"
fi
# Isolate the auth question from any action outcome: a non-SSE route must reject ?token=.
QGET=$(code "http://127.0.0.1:$PORT/api/usage?token=$TOKEN")
[[ "$QGET" == 401 ]] && pass "?token= is refused outside the event stream" \
  || fail "?token= is refused outside the event stream (got $QGET)"
pkill -f "web --host 127.0.0.1 --port $PORT" 2>/dev/null; sleep 1

echo "== token generation and reuse on a LAN bind =="
rm -f "$HOME_DIR/web_token"
( AID_HOME="$HOME_DIR" "$BIN" web --host 0.0.0.0 --port "$PORT" >"$HOME_DIR/gen.log" 2>&1 & )
for _ in $(seq 1 15); do sleep 1; [[ -s "$HOME_DIR/web_token" ]] && break; done
if [[ -s "$HOME_DIR/web_token" ]]; then
  pass "a LAN bind with no --token generates one"
  GEN=$(cat "$HOME_DIR/web_token")
  [[ ${#GEN} -ge 32 ]] && pass "generated token is at least 32 chars" || fail "generated token is at least 32 chars (got ${#GEN})"
  MODE=$(stat -f '%Lp' "$HOME_DIR/web_token" 2>/dev/null || stat -c '%a' "$HOME_DIR/web_token")
  [[ "$MODE" == 600 ]] && pass "token file is 0600" || fail "token file is 0600 (got $MODE)"
  [[ "$(code "http://127.0.0.1:$PORT/api/fleet" -H "Authorization: Bearer $GEN")" == 200 ]] \
    && pass "the generated token authenticates" || fail "the generated token authenticates"
  pkill -f "web --host 0.0.0.0 --port $PORT" 2>/dev/null; sleep 1
  ( AID_HOME="$HOME_DIR" "$BIN" web --host 0.0.0.0 --port "$PORT" >"$HOME_DIR/reuse.log" 2>&1 & )
  sleep 4
  [[ "$(code "http://127.0.0.1:$PORT/api/fleet" -H "Authorization: Bearer $GEN")" == 200 ]] \
    && pass "a restart reuses the persisted token" || fail "a restart reuses the persisted token"
else
  fail "a LAN bind with no --token generates one (no web_token written; log: $(head -1 "$HOME_DIR/gen.log"))"
fi

echo "== latency: /api/fleet paints the main window on every open ==" 
pkill -f "web --host 0.0.0.0 --port $PORT" 2>/dev/null; sleep 1
if start_loopback; then
  T_FLEET=$(curl -s -o /dev/null -w '%{time_total}' --max-time 60 "http://127.0.0.1:$PORT/api/fleet" -H "Authorization: Bearer $TOKEN")
  UNDER=$(python3 -c "print(1 if float('$T_FLEET') < 0.35 else 0)")
  [[ "$UNDER" == 1 ]] && pass "/api/fleet under 350ms (${T_FLEET}s)" || fail "/api/fleet under 350ms (took ${T_FLEET}s)"
  T_AGENTS=$(curl -s -o /dev/null -w '%{time_total}' --max-time 60 "http://127.0.0.1:$PORT/api/agents" -H "Authorization: Bearer $TOKEN")
  UNDER_A=$(python3 -c "print(1 if float('$T_AGENTS') < 0.12 else 0)")
  [[ "$UNDER_A" == 1 ]] && pass "/api/agents under 120ms (${T_AGENTS}s)" || fail "/api/agents under 120ms (took ${T_AGENTS}s)"
  T_TASKS=$(curl -s -o /dev/null -w '%{time_total}' --max-time 60 "http://127.0.0.1:$PORT/api/tasks" -H "Authorization: Bearer $TOKEN")
  UNDER_T=$(python3 -c "print(1 if float('$T_TASKS') < 0.15 else 0)")
  [[ "$UNDER_T" == 1 ]] && pass "/api/tasks under 150ms (${T_TASKS}s)" || fail "/api/tasks under 150ms (took ${T_TASKS}s)"
else
  fail "/api/fleet latency check could not start the server"
fi

echo
echo "passed $PASS, failed $FAIL, skipped $SKIP"
[[ $FAIL -eq 0 && $SKIP -eq 0 ]] || exit 1
