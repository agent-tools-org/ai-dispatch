#!/usr/bin/env python3
"""Synthetic app-server replay for Rust dispatch tests; never starts a model."""
import json
import os
import sys
import time
import subprocess

def capture(value):
    path = os.environ.get("AID_FAKE_PROVIDER_CAPTURE")
    if path:
        with open(path, "a", encoding="utf-8") as output:
            output.write(json.dumps(value) + "\n")


capture({"argv": sys.argv[1:], "provider_env": os.environ.get("OPENCODE_CONFIG"),
         "header_env": os.environ.get("PROVIDER_HEADER")})

if "--help" in sys.argv:
    print("app-server stdio transport")
    sys.exit(0)

with open(os.environ["AID_FAKE_FIXTURE"], encoding="utf-8") as fixture_file:
    rows = [json.loads(line) for line in fixture_file]
mode = os.environ.get("AID_FAKE_MODE", "accept")
if sys.argv[1] == "exec":
    assert mode != "write-config", "second launch must keep app-server steering"
    if mode == "reject-resume":
        assert "resume" not in sys.argv and "retry prompt" in sys.argv
    if mode == "stderr-init" and "resume" in sys.argv:
        with open(os.environ["AID_FAKE_ORIGIN"], encoding="utf-8") as saved:
            assert os.path.isdir(saved.read())
    assert "--ignore-user-config" in sys.argv
    assert os.path.exists(os.path.join(os.environ["CODEX_HOME"], "config.toml"))
    print(json.dumps({"type":"item.completed","item":{"type":"agent_message","text":"STEERED"}}))
    print(json.dumps({"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":0,
                                                      "output_tokens":1}}), flush=True)
    if "-o" in sys.argv:
        with open(sys.argv[sys.argv.index("-o")+1], "w", encoding="utf-8") as output_file:
            output_file.write("STEERED")
    sys.exit(0)



def send(message):
    print(json.dumps(message), flush=True)


def response(request, result):
    send({"id": request["id"], "result": result})


def notification(method):
    return next(row["msg"] for row in rows if row["dir"] == "in"
                and row["msg"].get("method") == method)


def finish(status="completed"):
    messages = [row["msg"] for row in rows if row["dir"] == "in"]
    final = [message for message in messages if message.get("method") == "item/completed"
             and message["params"]["item"].get("text") == "STEERED"][0]
    send(final)
    for message in messages:
        if message.get("method") == "thread/tokenUsage/updated":
            send(message)
    completed = notification("turn/completed")
    completed["params"]["turn"]["status"] = status
    send(completed)
    if mode == "write-config":
        with open(os.path.join(os.environ["CODEX_HOME"], "config.toml"), "w", encoding="utf-8") as config:
            config.write('[projects."synthetic"]\ntrust_level = "trusted"\n')


for raw in sys.stdin:
    request = json.loads(raw)
    capture(request)
    method = request.get("method")
    if method == "initialize":
        if mode == "stderr-init":
            print("synthetic startup diagnostic", file=sys.stderr, flush=True)
            sys.exit(1)
        if mode == "slow-init":
            time.sleep(float(os.environ.get("AID_FAKE_DELAY", "11")))
        if mode == "invalid":
            print("invalid json", flush=True)
            continue
        if mode != "silent-init":
            response(request, {"userAgent": "fixture"})
    elif method == "initialized":
        pass
    elif method in ("thread/start", "thread/resume"):
        assert not os.path.exists(os.path.join(os.environ["CODEX_HOME"], "config.toml"))
        assert os.path.islink(os.path.join(os.environ["CODEX_HOME"], "AGENTS.md"))
        if mode == "slow-resume":
            time.sleep(11)
        if mode == "silent-thread":
            continue
        if method == "thread/resume":
            if mode == "reject-resume":
                send({"id":request["id"],"error":{"code":-32600,"message":"no rollout found"}})
                continue
            if mode == "persist-origin":
                with open(os.environ["AID_FAKE_ORIGIN"], encoding="utf-8") as saved:
                    assert os.path.samefile(saved.read(), os.environ["CODEX_HOME"])
            else:
                assert request["params"]["threadId"] == "resume-thread"
            assert request["params"]["excludeTurns"] is True
        thread = next(row["msg"]["result"] for row in rows
                      if row["dir"] == "in" and row["msg"].get("id") == 2)
        if mode == "persist-origin" and method == "thread/start":
            with open(os.environ["AID_FAKE_ORIGIN"], "w", encoding="utf-8") as saved:
                saved.write(os.environ["CODEX_HOME"])
        response(request, thread)
    elif method == "turn/start":
        assert request["params"]["input"][0]["type"] == "text"
        if mode == "stderr-start":
            print("synthetic turn-start diagnostic", file=sys.stderr, flush=True)
            sys.exit(1)
        if mode == "silent-turn":
            continue
        if mode == "reject-start":
            send({"id":request["id"],"error":{"code":-32600,"message":"turn not started"}})
            continue
        started = notification("turn/started")
        response(request, {"turn": started["params"]["turn"]})
        send(started)
        if mode == "stderr-turn":
            print("synthetic turn diagnostic", file=sys.stderr, flush=True)
            sys.exit(1)
        if mode == "busy":
            send({"method":"item/started","params":{"item":{"type":"commandExecution","command":"busy"}}})
            subprocess.run([sys.executable,"-c","import time; end=time.monotonic()+2; "
                            "exec('while time.monotonic()<end: pass')"], check=True)
            finish()
        if mode == "hung":
            send({"method":"item/started","params":{"item":{"type":"commandExecution","command":"idle"}}})
        if mode == "first-output":
            time.sleep(0.6)
            finish()
        if mode == "approvals":
            for index, approval in enumerate(["item/commandExecution/requestApproval",
                    "item/fileChange/requestApproval", "item/permissions/requestApproval",
                    "mcpServer/elicitation/request", "item/tool/requestUserInput", "item/tool/call",
                    "execCommandApproval", "applyPatchApproval"]):
                send({"id":f"approval-{index}","method":approval,"params":{}})
                answer = json.loads(sys.stdin.readline())
                assert answer["id"] == f"approval-{index}" and "result" in answer
                result = answer["result"]
                if index < 2:
                    assert result["decision"] == "decline"
                elif index == 2:
                    assert result["permissions"] == {}
                elif index == 3:
                    assert result["action"] == "cancel"
                elif index == 4:
                    assert result["answers"] == {}
                elif index == 5:
                    assert result["success"] is False
                else:
                    assert "denied" in result["decision"]
            finish()
        if mode == "unknown-request":
            send({"id":"unknown","method":"account/chatgptAuthTokens/refresh","params":{}})
            answer = json.loads(sys.stdin.readline())
            assert "error" in answer
            finish()
        if mode == "exit-final":
            finish()
            sys.exit(7)
        if mode == "corrupt-final":
            finish()
            print("invalid json after terminal", flush=True)
        if mode == "late-final":
            send(notification("turn/completed"))
            time.sleep(0.3)
            finish()
        if mode == "eof":
            sys.exit(0)
        if mode in ("complete", "slow-init", "slow-resume", "flush", "persist-origin", "write-config"):
            finish()
    elif method == "turn/steer":
        assert request["params"]["expectedTurnId"] == notification("turn/started")["params"]["turn"]["id"]
        assert request["params"]["input"][0]["type"] == "text"
        if mode == "reject":
            send({"id": request["id"], "error": {"code": -32600,
                  "message": "non-steerable review/compact turn"}})
        elif mode != "silent-steer":
            if mode == "late-steer":
                time.sleep(11)
            turn_id = request["params"]["expectedTurnId"] if mode != "mismatch" else "wrong-turn"
            response(request, {"turnId": turn_id})
        finish()
    elif method == "turn/interrupt":
        assert request["params"]["turnId"] == notification("turn/started")["params"]["turn"]["id"]
        with open(os.environ["AID_FAKE_INTERRUPTED"], "w", encoding="utf-8") as marker:
            marker.write("interrupted")
        if mode == "slow-interrupt":
            time.sleep(2.5)
        response(request, {})
        finish("interrupted")
    else:
        raise AssertionError(f"Unexpected method {method}")

if mode in ("flush", "late-final"):
    with open(os.environ["AID_FAKE_FLUSHED"], "w", encoding="utf-8") as flushed:
        flushed.write("persisted after stdin EOF")
