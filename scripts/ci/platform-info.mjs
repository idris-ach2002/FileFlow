#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import os from 'node:os';

function version(command, args = ['--version']) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error || result.status !== 0) return 'unavailable';
  return (result.stdout || result.stderr || '').trim().split(/\r?\n/)[0];
}

function pnpmVersion() {
  const pnpmCli = process.env.npm_execpath;

  if (pnpmCli) {
    const result = spawnSync(
      process.execPath,
      [pnpmCli, '--version'],
      { encoding: 'utf8' },
    );

    if (!result.error && result.status === 0) {
      return (result.stdout || result.stderr || '')
        .trim()
        .split(/\r?\n/)[0];
    }
  }

  const result = spawnSync(
    'pnpm',
    ['--version'],
    {
      encoding: 'utf8',
      shell: process.platform === 'win32',
    },
  );

  if (result.error || result.status !== 0) {
    return 'unavailable';
  }

  return (result.stdout || result.stderr || '')
    .trim()
    .split(/\r?\n/)[0];
}

console.log(JSON.stringify({
  platform: process.platform,
  arch: process.arch,
  release: os.release(),
  node: process.version,
  pnpm: pnpmVersion(),
  rustc: version(process.platform === 'win32' ? 'rustc.exe' : 'rustc'),
  cargo: version(process.platform === 'win32' ? 'cargo.exe' : 'cargo'),
}, null, 2));
