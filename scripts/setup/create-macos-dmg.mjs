#!/usr/bin/env node
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function run(program, args, options = {}) {
  const result = spawnSync(program, args, { encoding: 'utf8', ...options });
  if (result.error || result.status !== 0) {
    const details = [result.stderr, result.stdout, result.error?.message].filter(Boolean).join('\n').trim();
    throw new Error(`${program} ${args.join(' ')} failed${details ? `:\n${details}` : ''}`);
  }
  return result;
}

function sleep(seconds) {
  run('/bin/sleep', [String(seconds)]);
}

export function createMacosSetupDmg({ target, targetDirectory, project = resolve(root, 'setup-tauri') }) {
  if (process.platform !== 'darwin') return null;
  if (!/^(aarch64|x86_64)-apple-darwin$/.test(target)) {
    throw new Error(`unsupported macOS target for Setup DMG: ${target}`);
  }

  const config = JSON.parse(readFileSync(resolve(project, 'tauri.conf.json'), 'utf8'));
  const version = config.version;
  const arch = target.startsWith('aarch64') ? 'aarch64' : 'x64';
  const bundleRoot = resolve(targetDirectory, target, 'release', 'bundle');
  const app = join(bundleRoot, 'macos', 'FileFlowSetup.app');
  const dmgDir = join(bundleRoot, 'dmg');
  const dmg = join(dmgDir, `FileFlowSetup_${version}_${arch}.dmg`);
  if (!existsSync(app)) throw new Error(`FileFlowSetup.app missing before DMG creation: ${app}`);
  mkdirSync(dmgDir, { recursive: true });
  rmSync(dmg, { force: true });

  const stage = mkdtempSync(join(tmpdir(), 'fileflow-setup-dmg-'));
  try {
    run('ditto', [app, join(stage, basename(app))]);
    symlinkSync('/Applications', join(stage, 'Applications'));

    let lastError;
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        rmSync(dmg, { force: true });
        console.log(`[setup-dmg] hdiutil attempt ${attempt}/2 -> ${dmg}`);
        run('hdiutil', [
          'create', '-volname', 'FileFlow Setup', '-srcfolder', stage,
          '-ov', '-format', 'UDZO', '-imagekey', 'zlib-level=9', dmg,
        ]);
        run('hdiutil', ['verify', dmg]);
        console.log(`[setup-dmg] verified ${dmg}`);
        return dmg;
      } catch (error) {
        lastError = error;
        console.error(`[setup-dmg] attempt ${attempt} failed: ${error.message}`);
        sleep(2);
      }
    }
    throw lastError || new Error('unknown hdiutil failure');
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = new Map();
  for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
  const target = args.get('--target');
  const targetDirectory = resolve(root, args.get('--target-directory') || process.env.FILEFLOW_SETUP_TARGET_DIR || 'target/fileflow-setup');
  if (!target) throw new Error('usage: create-macos-dmg.mjs --target <apple-target> [--target-directory <path>]');
  createMacosSetupDmg({ target, targetDirectory });
}
