#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

const [script, ...args] = process.argv.slice(2);
if (!script) {
  console.error('usage: node scripts/release/run-python.mjs <script.py> [args...]');
  process.exit(2);
}

const candidates = process.platform === 'win32'
  ? [['python', []], ['py', ['-3']], ['python3', []]]
  : [['python3', []], ['python', []]];

for (const [command, prefix] of candidates) {
  const probe = spawnSync(command, [...prefix, '--version'], { stdio: 'ignore' });
  if (!probe.error && probe.status === 0) {
    const result = spawnSync(command, [...prefix, resolve(script), ...args], { stdio: 'inherit' });
    if (result.error) throw result.error;
    process.exit(result.status ?? 1);
  }
}

console.error('Python 3 is required for FileFlow release tooling.');
process.exit(1);
