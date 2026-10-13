#!/bin/sh
# Controlled Codex delivery fixture for native permission profiles and staged reports.
# Keeps fresh/resume, short-answer and missing-delivery scenarios deterministic.
if [ "$1" = "--version" ]; then
  if [ "$FAKE_CODEX_LEGACY" = "1" ]; then
    echo "codex-cli 0.146.0"
  else
    echo "codex-cli 0.147.0"
  fi
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  echo "      --ignore-user-config"
  exit 0
fi
case " $* " in
  *" --ignore-user-config "*) ;;
  *) echo "error: missing '--ignore-user-config'" >&2; exit 2 ;;
esac
case " $* " in
  *' default_permissions="aid-read-only" '*)
    read_only=true
    case "$*" in *':read-only'*) ;; *) exit 2 ;; esac
    ;;
  *' default_permissions="aid-write" '*)
    read_only=false
    case "$*" in *':workspace'*) ;; *) exit 2 ;; esac
    ;;
  *) echo 'error: missing native permissions profile' >&2; exit 2 ;;
esac
case " $* " in
  *" --full-auto "*|*" --approve-for-me "*) exit 2 ;;
esac
final_delivery() {
  if "$read_only"; then printf '%s\n' "$1" > "$TMPDIR/agent-result.md"; fi
  printf '%s%s%s\n' '{"type":"item.completed","item":{"id":"final",' \
    '"type":"agent_message","text":"' "$1\"}}"
  printf '%s%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,' \
    "\"cached_input_tokens\":10,\"output_tokens\":$2}}"
}
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"019e3e49-6b83-7563-a3d8-b51a3a716dd1"}'
  if [ "$FAKE_CODEX_HOLLOW_RESUME" = "1" ]; then
    printf '%s%s\n' \
      '{"type":"item.completed","item":{"id":"work-again","type":"command_execution",' \
      '"command":"inspect again","aggregated_output":"","exit_code":0}}'
    printf '%s%s\n' \
      '{"type":"turn.completed","usage":{"input_tokens":20,' \
      '"cached_input_tokens":10,"output_tokens":8000}}'
    exit 0
  fi
  final_delivery 'Recovery complete.' 80
  exit 0
fi
printf '%s\n' '{"type":"thread.started","thread_id":"019e3e49-6b83-7563-a3d8-b51a3a716dd1"}'
if [ "$FAKE_CODEX_SHORT_FINAL" = "1" ]; then
  final_delivery 'ok' 1
  exit 0
fi
if [ "$FAKE_CODEX_FINAL_ON_FRESH" = "1" ]; then
  final_delivery 'Completed and verified.' 80
  exit 0
fi
printf '%s%s%s%s\n' \
  '{"type":"item.completed","item":{"id":"progress","type":"agent_message","text":"' \
  'I am investigating the task now. I will inspect the available evidence' \
  ' and provide the final report after the tool work is complete.' \
  ' This progress update is deliberately substantive but is not a final answer."}}'
printf '%s%s\n' \
  '{"type":"item.completed","item":{"id":"work","type":"command_execution",' \
  '"command":"inspect evidence","aggregated_output":"","exit_code":0}}'
printf '%s%s\n' \
  '{"type":"turn.completed","usage":{"input_tokens":100,' \
  '"cached_input_tokens":50,"output_tokens":9000}}'
