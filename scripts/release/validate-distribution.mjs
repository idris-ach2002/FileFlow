#!/usr/bin/env node
import { existsSync, readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { applicationBundleRoot, setupBundleRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}
const target = args.get('--target');
const strict = process.argv.includes('--strict');
const requireSetup = process.argv.includes('--require-setup');
if (!target) {
  throw new Error('usage: validate-distribution.mjs --target <target> [--strict] [--require-setup]');
}

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? [path, ...walk(path)] : [path];
  });
}
function filesBelow(directory, label) {
  if (!existsSync(directory)) throw new Error(`${label} bundle root missing: ${directory}`);
  return walk(directory);
}
function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(' ')} failed: ${result.stderr || result.stdout || result.error}`);
  }
}

const applicationFiles = filesBelow(applicationBundleRoot(root, target), 'application');
const setupFiles = requireSetup
  ? filesBelow(setupBundleRoot(root, target), 'Setup')
  : [];

if (process.platform === 'darwin') {
  const app = applicationFiles.find((path) => path.endsWith('FileFlow.app'));
  const dmg = applicationFiles.find((path) => path.endsWith('.dmg')
    && !/fileflow[ _.-]?setup/i.test(basename(path)));
  if (!app || !dmg) throw new Error('macOS application APP/DMG missing');
  run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app]);
  if (strict) {
    run('xcrun', ['stapler', 'validate', app]);
    run('xcrun', ['stapler', 'validate', dmg]);
    run('spctl', ['--assess', '--type', 'execute', '--verbose=2', app]);
  }
  if (requireSetup) {
    const setupApp = setupFiles.find((path) => path.endsWith('FileFlowSetup.app'));
    const setupDmg = setupFiles.find((path) => path.endsWith('.dmg')
      && /fileflow[ _.-]?setup/i.test(basename(path)));
    if (!setupApp || !setupDmg) throw new Error('macOS Setup APP/DMG missing');
    run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', setupApp]);
    if (strict) {
      run('xcrun', ['stapler', 'validate', setupApp]);
      run('xcrun', ['stapler', 'validate', setupDmg]);
      run('spctl', ['--assess', '--type', 'execute', '--verbose=2', setupApp]);
    }
  }
} else if (process.platform === 'win32') {
  const installers = applicationFiles.filter((path) => /\.(exe|msi)$/i.test(path)
    && !/fileflow[ _.-]?setup/i.test(basename(path)));
  if (!installers.length) throw new Error('Windows application installers missing');
  if (strict) {
    for (const path of installers) {
      const escaped = path.replaceAll("'", "''");
      run('powershell', ['-NoProfile', '-Command', `$s=Get-AuthenticodeSignature -LiteralPath '${escaped}'; if ($s.Status -ne 'Valid') { Write-Error $s.Status; exit 2 }`]);
    }
  }
  if (requireSetup) {
    const setup = setupFiles.find((path) => /\.exe$/i.test(path)
      && /fileflow[ _.-]?setup/i.test(basename(path))
      && !/cli/i.test(basename(path)));
    if (!setup) throw new Error('Windows Setup EXE missing');
    if (strict) {
      const escaped = setup.replaceAll("'", "''");
      run('powershell', ['-NoProfile', '-Command', `$s=Get-AuthenticodeSignature -LiteralPath '${escaped}'; if ($s.Status -ne 'Valid') { Write-Error $s.Status; exit 2 }`]);
    }
  }
} else {
  const appImage = applicationFiles.find((path) => path.toLowerCase().endsWith('.appimage')
    && !/fileflow[ _.-]?setup/i.test(basename(path)));
  if (!appImage) throw new Error('Linux application AppImage missing');
  run('file', [appImage]);
  if (requireSetup) {
    const setup = setupFiles.find((path) => path.toLowerCase().endsWith('.appimage')
      && /fileflow[ _.-]?setup/i.test(basename(path)));
    if (!setup) throw new Error('Linux Setup AppImage missing');
    run('file', [setup]);
  }
}

console.log(`[distribution] validated ${target}${requireSetup ? ' + Setup' : ''}${strict ? ' (strict signatures/notarization)' : ''}`);
