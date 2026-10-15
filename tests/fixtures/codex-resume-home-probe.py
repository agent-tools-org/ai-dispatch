#!/usr/bin/env python3
"""Reproduce Codex 0.162.1 resume path persistence using only synthetic local state."""
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading


class Client:
    def __init__(self, home):
        env = dict(os.environ, CODEX_HOME=str(home))
        env.pop("CODEX_SQLITE_HOME", None)
        self.process = subprocess.Popen(
            ["codex", "app-server", "-c", 'model_provider="offline"', "-c",
             'model_providers.offline={name="offline",base_url="http://127.0.0.1:9",'
             'wire_api="responses"}'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, env=env,
        )
        self.messages = queue.Queue()
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()
        self.request(1, "initialize", {"clientInfo": {"name": "aid-probe", "version": "0"}})
        self.send({"method": "initialized"})

    def read(self):
        for line in self.process.stdout:
            self.messages.put(json.loads(line))
        self.messages.put(None)

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def request(self, identifier, method, params):
        import time
        self.send({"id": identifier, "method": method, "params": params})
        deadline = time.monotonic() + 10
        while True:
            message = self.messages.get(timeout=max(0.001, deadline - time.monotonic()))
            if message is None:
                raise RuntimeError("app-server exited before responding")
            if message.get("id") == identifier:
                return message
            if time.monotonic() >= deadline:
                raise TimeoutError(method)

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
            raise
        finally:
            self.process.stdout.close()
            self.reader.join(timeout=1)


def mirror(real, path):
    path.mkdir()
    for entry in real.iterdir():
        if entry.name != "config.toml" and not entry.name.startswith("config.toml."):
            (path / entry.name).symlink_to(entry)
    return path


def probe(root):
    real = root / "real"
    real.mkdir()
    (real / "sessions").mkdir()
    # Initialize synthetic database files before mirroring them, as on a warm Codex home.
    client = Client(real)
    try:
        response = client.request(2, "thread/start", {
            "cwd": str(root), "sandbox": "read-only", "approvalPolicy": "never"})
        assert "result" in response, response
    finally:
        client.close()
    origin = mirror(real, root / "origin")
    client = Client(origin)
    try:
        response = client.request(2, "thread/start", {
            "cwd": str(root), "sandbox": "read-only", "approvalPolicy": "never"})
        thread = response["result"]["thread"]["id"]
        response = client.request(3, "turn/start", {
            "threadId": thread, "input": [{"type": "text", "text": "synthetic probe"}]})
        assert "result" in response, response
    finally:
        client.close()
    assert list((real / "sessions").rglob(f"*-{thread}.jsonl")), "rollout must survive"
    shutil.rmtree(origin)
    client = Client(mirror(real, root / "retry"))
    try:
        params = {"threadId": thread, "excludeTurns": True}
        response = client.request(2, "thread/resume", params)
        assert "no rollout found" in response["error"]["message"], response
        mirror(real, origin)
        response = client.request(3, "thread/resume", params)
        assert response["result"]["thread"]["id"] == thread, response
    finally:
        client.close()
    shutil.rmtree(origin)
    assert not origin.exists()
    print("PASS: deleted origin fails; restored origin resumes; temporary home cleaned (3 checks)")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="aid-resume-probe-") as temporary:
        probe(Path(temporary).resolve())
