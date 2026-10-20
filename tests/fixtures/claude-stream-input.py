#!/usr/bin/env python3
# Fake Claude transport using SDK-schema replay fixtures; no model request.
# Exercises multiple results, stdin EOF, and unacknowledged delivery.
import json
import pathlib
import select
import sys

mode, fixture, output = sys.argv[1:4]
args = sys.argv[4:]
assert args[0] == '-p' and args[1] == '--output-format'
assert '--input-format' in args and '--replay-user-messages' in args
rows = [json.loads(line) for line in pathlib.Path(fixture).read_text().splitlines()]

def emit(row):
    print(json.dumps(row), flush=True)

initial = json.loads(sys.stdin.readline())
assert initial['message']['content'] == 'initial prompt'
assert initial['uuid'] == rows[0]['uuid']
emit(rows[0])
emit(rows[1])
steer = json.loads(sys.stdin.readline())
pathlib.Path(output).write_text(json.dumps(steer))
rows[2]['uuid'] = steer['uuid']
if mode == 'ack':
    emit(rows[2])
    emit(rows[2])
if mode == 'eof':
    sys.exit(0)
assert not select.select([sys.stdin], [], [], 0.3)[0], 'stdin closed before final result'
emit(rows[3])
assert sys.stdin.read() == ''
pathlib.Path(output + '.closed').touch()
