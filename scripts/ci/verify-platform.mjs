#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
const tauri = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
const frontend = JSON.parse(readFileSync(resolve(root, 'frontend/package.json'), 'utf8'));
const toolchain = readFileSync(resolve(root, 'rust-toolchain.toml'), 'utf8');
const workspaceCargo = readFileSync(resolve(root, 'Cargo.toml'), 'utf8');
const workspaceSection = workspaceCargo
  .split('[workspace.package]')[1]
  ?.split(/\n\[/)[0];
const workspaceVersion = workspaceSection
  ?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

const expectedRust = toolchain.match(/channel\s*=\s*"([^"]+)"/)?.[1];

function fail(message) {
  console.error(`[preflight] ${message}`);
  process.exit(2);
}
function output(command, args = ['--version']) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8' });
  if (result.error || result.status !== 0) fail(`${command} is unavailable`);
  return (result.stdout || result.stderr || '').trim();
}
function tuple(version) { return version.split('.').map((v) => Number.parseInt(v, 10) || 0); }
function cmp(a,b) { for (let i=0;i<3;i++) { if ((a[i]??0)!==(b[i]??0)) return (a[i]??0)-(b[i]??0); } return 0; }
function nodeAllowed(version) {
  const v=tuple(version.replace(/^v/,''));
  return (v[0]===22 && cmp(v,[22,22,3])>=0) || (v[0]===24 && cmp(v,[24,15,0])>=0) || v[0]===26;
}

if (!nodeAllowed(process.version)) fail(`unsupported Node ${process.version}; expected ${pkg.engines.node}`);
const pnpmCmd = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
const rustcCmd = process.platform === 'win32' ? 'rustc.exe' : 'rustc';
const pnpmVersion = output(pnpmCmd).split(/\s+/)[0];
if (pnpmVersion !== '11.20.0') fail(`pnpm ${pnpmVersion}; expected 11.20.0`);
const rustVersion = output(rustcCmd).match(/rustc\s+(\d+\.\d+\.\d+)/)?.[1];
if (!rustVersion || rustVersion !== expectedRust) fail(`rustc ${rustVersion ?? '?'}; expected ${expectedRust}`);
if (new Set([pkg.version, frontend.version, tauri.version]).size !== 1) fail('package/frontend/Tauri versions are not synchronized');
if (!workspaceVersion || workspaceVersion !== pkg.version) fail(`Cargo workspace version ${workspaceVersion ?? '?'}; expected ${pkg.version}`);
if (!readFileSync(resolve(root, 'pnpm-lock.yaml'), 'utf8').startsWith("lockfileVersion: '9.0'")) fail('unexpected pnpm lockfile version');
if (!readFileSync(resolve(root, 'Cargo.lock'), 'utf8').includes(`name = "fileflow-desktop"\nversion = "${pkg.version}"`)) fail('Cargo.lock FileFlow version is stale');

console.log(`[preflight] FileFlow ${pkg.version}`);
console.log(`[preflight] ${process.platform}/${process.arch} Node ${process.version} pnpm ${pnpmVersion} Rust ${rustVersion}`);
