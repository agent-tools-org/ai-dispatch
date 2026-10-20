#!/usr/bin/env python3
# Production streaming transport fixture; no model calls or credentials.
# A file gate lets the observer verify unconfirmed delivery before the replay.
import json
import pathlib
import select
import sys
import subprocess
import time

mode, output = sys.argv[1:3]
output = pathlib.Path(output)
args = sys.argv[3:]
assert args[:3] == ['-p', '--output-format', 'stream-json']
assert '--input-format' in args and '--replay-user-messages' in args

def emit(row):
    print(json.dumps(row), flush=True)

def replay(row):
    emit(dict(row, isReplay=True))

initial = json.loads(sys.stdin.readline())
assert initial['message']['content'] == 'initial prompt'
if mode == 'startup':
    select.select([], [], [], 5)
    sys.exit(0)
replay(initial)
emit({'type': 'system', 'subtype': 'init'})
if mode in ('partial', 'idle', 'cost', 'stderr', 'descendant'):
    if mode == 'partial':
        sys.stdout.write('partial raw output')
        sys.stdout.flush()
    if mode == 'cost':
        emit({'type': 'result', 'subtype': 'success', 'total_cost_usd': 2,
              'usage': {'input_tokens': 3, 'output_tokens': 4}, 'is_error': False})
    if mode in ('idle', 'stderr', 'descendant'):
        emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'working'}]}})
    if mode == 'descendant':
        rbox = output.parent / 'rbox'
        rbox.write_text('#!/bin/sh\nsleep 2\n')
        rbox.chmod(0o700)
        subprocess.run([str(rbox), 'task', '--wait'], check=True)
    elif mode == 'stderr':
        for _ in range(6):
            print('stderr progress', file=sys.stderr, flush=True)
            select.select([], [], [], 0.25)
    else:
        select.select([], [], [], 5)
    emit({'type': 'result', 'subtype': 'success', 'result': 'final result', 'is_error': False})
    sys.exit(0)
if mode == 'blocked':
    # Keep the read end open without draining a steer larger than pipe capacity.
    select.select([], [], [], 5.5)
    sys.stdin.read()
else:
    steer = json.loads(sys.stdin.readline())
    output.write_text(json.dumps(steer))
    deadline = time.monotonic() + 5
    while not output.with_suffix('.replay').exists():
        assert time.monotonic() < deadline, 'observer never released replay'
        select.select([], [], [], 0.01)
    replay(steer)
    assert not select.select([sys.stdin], [], [], 0.3)[0], 'stdin closed before result'
emit({'type': 'result', 'subtype': 'success', 'result': 'final result', 'is_error': False})
assert sys.stdin.read() == ''
output.with_suffix('.closed').touch()
