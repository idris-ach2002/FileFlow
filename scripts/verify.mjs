#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const isWindows = process.platform === 'win32';
const pnpm = isWindows ? 'pnpm.cmd' : 'pnpm';
const cargo = isWindows ? 'cargo.exe' : 'cargo';
const rustc = isWindows ? 'rustc.exe' : 'rustc';

function output(command, args = []) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.stderr.write(result.stderr || result.stdout || '');
    process.exit(result.status ?? 1);
  }
  return (result.stdout || '').trim();
}

function run(label, command, args) {
  console.log(`\n${label}`);
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

console.log('\n== FileFlow verification ==');
console.log(`Node:  ${process.version}`);
console.log(`pnpm:  ${output(pnpm, ['--version'])}`);
console.log(`Rust:  ${output(rustc, ['--version'])}`);

run('1/6 Angular production build', pnpm, ['run', 'frontend:build']);
run('2/6 Angular tests', pnpm, ['run', 'frontend:test']);
run('3/6 Rust formatting', cargo, ['fmt', '--all', '--', '--check']);
run('4/6 Rust workspace check', cargo, ['check', '--workspace', '--locked']);
run('5/6 Rust tests', cargo, ['test', '--workspace', '--locked']);
run('6/6 Clippy (warnings are errors)', cargo, [
  'clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings',
]);

console.log('\nFileFlow verification passed.');
