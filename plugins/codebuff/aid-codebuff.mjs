#!/usr/bin/env node
/**
 * Headless Codebuff bridge that streams SDK events as codex JSONL.
 * Exports: CLI `aid-codebuff` with prompt, cwd, model, mode, and read-only flags.
 * Deps: @codebuff/sdk plus node:util/parseArgs for argument parsing.
 */
import pkg from '@codebuff/sdk';
const { CodebuffClient } = pkg;
import { parseArgs } from 'node:util';

const args = parseArgs({
  allowPositionals: true,
  options: {
    cwd: { type: 'string' },
    model: { type: 'string' },
    mode: { type: 'string', default: 'DEFAULT' },
    'read-only': { type: 'boolean', default: false },
  },
});

const prompt = args.positionals.join(' ').trim();
if (!prompt) {
  console.error('Usage: aid-codebuff <prompt> [--cwd <dir>] [--model <model>] [--mode <DEFAULT|FREE|MAX>]');
  process.exit(1);
}
const apiKey = process.env.CODEBUFF_API_KEY;
if (!apiKey) {
  console.error('Missing CODEBUFF_API_KEY. Get one at: https://www.codebuff.com/api-keys');
  process.exit(1);
}

const cwd = args.values.cwd || process.cwd();
const emit = (payload) => process.stdout.write(`${JSON.stringify(payload)}\n`);

const client = new CodebuffClient({
  apiKey,
  cwd,
  onError: (err) => {
    emit({ type: 'error', message: err?.message ?? String(err) });
  },
});

const usageTotals = { inputTokens: 0, outputTokens: 0 };

const handleEvent = (event) => {
  if (!event || typeof event.type !== 'string') return;
  switch (event.type) {
    case 'start':
      emit({ type: 'item.started', item: { type: 'agent_message', text: `[codebuff] agent started` } });
      break;
    case 'text':
    case 'reasoning_delta':
      if (event.text) emit({ type: 'item.completed', item: { type: 'agent_message', text: event.text } });
      break;
    case 'tool_call': {
      const input = event.input && typeof event.input === 'object' ? ` ${JSON.stringify(event.input).slice(0, 200)}` : '';
      emit({ type: 'item.started', item: { type: 'command_execution', command: `${event.toolName || 'tool'}${input}` } });
      break;
    }
    case 'tool_result': {
      const out = Array.isArray(event.output)
        ? event.output.map(e => e.type === 'json' ? JSON.stringify(e.value).slice(0, 500) : `[${e.type}]`).join('\n')
        : '';
      emit({ type: 'item.completed', item: { type: 'command_execution', command: event.toolName || 'tool', exit_code: 0, aggregated_output: out } });
      break;
    }
    case 'usage':
      if (event.usage) {
        usageTotals.inputTokens = Math.max(usageTotals.inputTokens, event.usage.inputTokens ?? event.usage.input_tokens ?? 0);
        usageTotals.outputTokens = Math.max(usageTotals.outputTokens, event.usage.outputTokens ?? event.usage.output_tokens ?? 0);
      }
      break;
    case 'error':
      emit({ type: 'error', message: event.message || 'unknown error' });
      break;
    case 'finish':
      if (event.output) emit({ type: 'item.completed', item: { type: 'agent_message', text: `[codebuff] done` } });
      break;
  }
};

(async () => {
  try {
    const result = await client.run({
      agent: 'codebuff/base@latest',
      prompt,
      handleEvent,
      maxAgentSteps: 50,
    });
    emit({
      type: 'turn.completed',
      usage: { input_tokens: usageTotals.inputTokens, output_tokens: usageTotals.outputTokens, cached_input_tokens: 0 },
    });
    if (result?.output) {
      const out = typeof result.output === 'string' ? result.output : JSON.stringify(result.output);
      process.stderr.write(`[aid-codebuff] Output: ${out.slice(0, 300)}\n`);
    }
    client.closeConnection?.();
    process.exit(0);
  } catch (err) {
    emit({ type: 'error', message: err?.message ?? String(err) });
    client.closeConnection?.();
    process.exit(1);
  }
})();
