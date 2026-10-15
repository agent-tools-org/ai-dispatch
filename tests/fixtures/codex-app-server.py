#!/usr/bin/env python3
"""Synthetic app-server replay for Rust dispatch tests; never starts a model."""
import json
import os
import sys

if "--help" in sys.argv:
    print("app-server stdio transport")
    sys.exit(0)

with open(os.environ["AID_FAKE_FIXTURE"], encoding="utf-8") as fixture_file:
    rows = [json.loads(line) for line in fixture_file]
mode = os.environ.get("AID_FAKE_MODE", "accept")


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


for raw in sys.stdin:
    request = json.loads(raw)
    method = request.get("method")
    if method == "initialize":
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
        if mode == "silent-thread":
            continue
        if method == "thread/resume":
            assert request["params"]["threadId"] == "resume-thread"
        thread = next(row["msg"]["result"] for row in rows
                      if row["dir"] == "in" and row["msg"].get("id") == 2)
        response(request, thread)
    elif method == "turn/start":
        assert request["params"]["input"][0]["type"] == "text"
        started = notification("turn/started")
        response(request, {"turn": started["params"]["turn"]})
        send(started)
        if mode == "eof":
            sys.exit(0)
        if mode == "complete":
            finish()
    elif method == "turn/steer":
        assert request["params"]["expectedTurnId"] == notification("turn/started")["params"]["turn"]["id"]
        assert request["params"]["input"][0]["type"] == "text"
        if mode == "reject":
            send({"id": request["id"], "error": {"code": -32600,
                  "message": "non-steerable review/compact turn"}})
        elif mode != "silent-steer":
            turn_id = request["params"]["expectedTurnId"] if mode != "mismatch" else "wrong-turn"
            response(request, {"turnId": turn_id})
        finish()
    elif method == "turn/interrupt":
        assert request["params"]["turnId"] == notification("turn/started")["params"]["turn"]["id"]
        with open(os.environ["AID_FAKE_INTERRUPTED"], "w", encoding="utf-8") as marker:
            marker.write("interrupted")
        response(request, {})
        finish("interrupted")
    else:
        raise AssertionError(f"Unexpected method {method}")
