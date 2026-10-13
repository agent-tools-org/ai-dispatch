# Native Codex sandbox probe using a local Responses API stub.
# Runs adapter-generated exec/resume arguments with ambient legacy settings.
# Dependencies: Python standard library and the installed Codex executable.
import http.server
import json
import os
from pathlib import Path
import subprocess
import sys
import threading

repo, scratch, home = map(Path, sys.argv[1:4])
invocations = json.loads(sys.argv[4])
state = {"requests": 0, "command": ""}


def shell_call(tools, namespace=None):
    for tool in tools:
        if tool["type"] == "namespace":
            found = shell_call(tool["tools"], tool["name"])
            if found:
                return found
        name = tool.get("name", "")
        short = name.rsplit(".", 1)[-1]
        if short in ("exec_command", "shell_command", "shell"):
            key = "cmd" if short == "exec_command" else "command"
            value = ["/bin/sh", "-c", state["command"]] if short == "shell" else state["command"]
            return name, namespace, {key: value}
    return None


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        state["requests"] += 1
        if state["requests"] == 1:
            name, namespace, arguments = shell_call(request["tools"])
            item = {
                "type": "function_call", "id": "fc_probe", "call_id": "call_probe",
                "name": name, "arguments": json.dumps(arguments),
            }
            if namespace:
                item["namespace"] = namespace
        else:
            item = {
                "type": "message", "id": "msg_probe", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": "native probe complete", "annotations": []}],
            }
        events = [
            {"type": "response.created", "response": {"id": "resp_probe"}},
            {"type": "response.output_item.done", "output_index": 0, "item": item},
            {"type": "response.completed", "response": {
                "id": "resp_probe", "status": "completed", "output": [item],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
            }},
        ]
        body = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
worker = threading.Thread(target=server.serve_forever, daemon=True)
worker.start()
provider = {
    "name": "native-probe", "base_url": f"http://127.0.0.1:{server.server_port}/v1",
    "wire_api": "responses", "requires_openai_auth": False,
    "request_max_retries": 0, "stream_max_retries": 0,
}
env = dict(os.environ, CODEX_HOME=str(home), NO_PROXY="127.0.0.1,localhost")
env.pop("CODEX_API_KEY", None)
env.pop("OPENAI_API_KEY", None)
session = None
print(subprocess.check_output(["codex", "--version"], text=True).strip(), flush=True)
print("Remote tested HEAD: " + subprocess.check_output(
    ["git", "rev-parse", "HEAD"], text=True).strip(), flush=True)
try:
    for invocation in invocations:
        read_only = invocation["read_only"]
        (home / "config.toml").write_text(
            "sandbox_mode = '" + ("danger-full-access" if read_only else "read-only") + "'\n"
            + '[projects."' + str(repo) + '"]\ntrust_level = "trusted"\n'
        )
        blocked = repo / "probe-write.txt"
        allowed = scratch / "probe-write.txt"
        blocked.unlink(missing_ok=True)
        allowed.unlink(missing_ok=True)
        code = (
            "from pathlib import Path\n"
            + f"Path({str(allowed)!r}).write_text('allowed')\n"
            + f"try:\n Path({str(blocked)!r}).write_text('repo')\n"
            + "except PermissionError:\n pass\n"
        )
        import shlex
        state.update(requests=0, command="python3 -c " + shlex.quote(code))
        argv = [session if arg == "SESSION" else arg for arg in invocation["argv"]]
        settings = "model_providers.native-probe={" + ",".join(
            key + "=" + json.dumps(value) for key, value in provider.items()
        ) + "}"
        argv += ["-c", 'model_provider="native-probe"', "-c", settings]
        result = subprocess.run(["codex", *argv], cwd=repo, env=env,
                                capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stdout + result.stderr
        assert state["requests"] >= 2, result.stdout + result.stderr
        assert allowed.exists(), result.stdout + result.stderr
        assert blocked.exists() == (not read_only), result.stdout + result.stderr
        for line in result.stdout.splitlines():
            event = json.loads(line)
            if event.get("type") == "thread.started":
                session = event["thread_id"]
        assert session, result.stdout
        print(f"Native read_only={read_only}: scratch writable, repository writable={blocked.exists()}", flush=True)
finally:
    server.shutdown()
