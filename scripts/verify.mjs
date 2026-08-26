#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const isWindows = process.platform === 'win32';
const cargo = isWindows ? 'cargo.exe' : 'cargo';
const rustc = isWindows ? 'rustc.exe' : 'rustc';

function output(command, args = [], options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    ...options,
  });

  if (result.error) throw result.error;

  if (result.status !== 0) {
    process.stderr.write(result.stderr || result.stdout || '');
    process.exit(result.status ?? 1);
  }

  return (result.stdout || '').trim();
}

function pnpmInvocation(args = []) {
  const pnpmCli = process.env.npm_execpath;

  // `pnpm run verify` exposes the real pnpm JS entrypoint here.
  // Running it through the current Node executable works identically on
  // macOS, Linux and Windows and avoids spawning .cmd wrappers.
  if (pnpmCli) {
    return {
      command: process.execPath,
      args: [pnpmCli, ...args],
      options: {},
    };
  }

  // Direct invocation fallback:
  // node scripts/verify.mjs
  return {
    command: 'pnpm',
    args,
    options: {
      shell: isWindows,
    },
  };
}

function outputPnpm(args = []) {
  const invocation = pnpmInvocation(args);

  return output(
    invocation.command,
    invocation.args,
    invocation.options,
  );
}

function run(label, command, args, options = {}) {
  console.log(`\n${label}`);

  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    ...options,
  });

  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function runPnpm(label, args) {
  const invocation = pnpmInvocation(args);

  run(
    label,
    invocation.command,
    invocation.args,
    invocation.options,
  );
}

console.log('\n== FileFlow verification ==');
console.log(`Node:  ${process.version}`);
console.log(`pnpm:  ${outputPnpm(['--version'])}`);
console.log(`Rust:  ${output(rustc, ['--version'])}`);

runPnpm('1/11 Angular production build', ['run', 'frontend:build']);
runPnpm('2/11 Angular tests', ['run', 'frontend:test']);
run('3/11 Setup UI selection tests', process.execPath, ['scripts/setup/test-ui.cjs']);
run('4/11 Setup local artifact resolver', process.execPath, ['scripts/setup/test-local-source.mjs']);
run('5/11 Setup platform regression tests', process.execPath, ['scripts/setup/test-runtime-support.mjs']);
run('6/11 FileFlow branding invariants', process.execPath, ['scripts/release/verify-branding.mjs']);
run('7/11 Cloudflare download portal tests', process.execPath, ['website/scripts/test.mjs']);
run('8/11 Rust formatting', cargo, ['fmt', '--all', '--', '--check']);
run('9/11 Rust workspace check', cargo, ['check', '--workspace', '--locked']);
run('10/11 Rust tests', cargo, ['test', '--workspace', '--locked']);
run('11/11 Clippy (warnings are errors)', cargo, [
  'clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings',
]);

console.log('\nFileFlow verification passed.');
