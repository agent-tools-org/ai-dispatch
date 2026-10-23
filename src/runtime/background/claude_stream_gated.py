# Event-driven Claude stream fixture for loaded-box regression tests.
# The observer releases input gates; watchdog subjects never finish on their own.
import json
import pathlib
import select
import sys

mode, output = sys.argv[1:3]
output = pathlib.Path(output)

def emit(row):
    print(json.dumps(row), flush=True)

def gate():
    output.with_suffix('.ready').touch()
    while not output.with_suffix('.release').exists():
        select.select([], [], [], 0.01)

if mode == 'delayed':
    gate()
initial = json.loads(sys.stdin.readline())
if mode == 'delayed':
    output.write_text(json.dumps(initial))
if mode == 'startup':
    select.select([], [], [])
emit(dict(initial, isReplay=True))
emit({'type': 'system', 'subtype': 'init'})
if mode == 'partial':
    sys.stdout.write('partial raw output')
    sys.stdout.flush()
if mode == 'idle':
    emit({'type': 'assistant', 'message': {'content': [{'type': 'text', 'text': 'working'}]}})
if mode == 'cost':
    emit({'type': 'result', 'subtype': 'success', 'total_cost_usd': 2,
          'usage': {'input_tokens': 3, 'output_tokens': 4}, 'is_error': False})
if mode in ('partial', 'idle', 'cost'):
    select.select([], [], [])
if mode == 'blocked':
    gate()
    assert len(sys.stdin.read()) < 2 * 1024 * 1024, 'steer unexpectedly written in full'
else:
    assert initial['message']['content'] == 'x' * (256 * 1024)
emit({'type': 'result', 'subtype': 'success', 'result': 'final result', 'is_error': False})
assert sys.stdin.read() == ''
output.with_suffix('.closed').touch()
