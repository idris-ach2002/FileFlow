#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

function runChecked(program, args, options = {}) {
  const result = spawnSync(program, args, {
    stdio: 'inherit',
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${program} ${args.join(' ')} a échoué avec le code ${result.status ?? 'inconnu'}`);
  }
}

export function forwardedValue(forwarded, flag) {
  const index = forwarded.indexOf(flag);
  return index >= 0 ? forwarded[index + 1] || '' : '';
}

export function macosSetupBuild(forwarded, platform = process.platform) {
  const target = forwardedValue(forwarded, '--target');
  return platform === 'darwin'
    && forwarded[0] === 'build'
    && /^(aarch64|x86_64)-apple-darwin$/.test(target)
    ? target
    : null;
}

function removeFlagWithValue(args, flag) {
  const result = [];
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === flag) {
      index += 1;
      continue;
    }
    result.push(args[index]);
  }
  return result;
}

export function macosBuildPipeline(forwarded, platform = process.platform) {
  const target = macosSetupBuild(forwarded, platform);
  if (!target) return null;

  const requestedBundles = forwardedValue(forwarded, '--bundles')
    .split(',')
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean);
  const wantsDmg = requestedBundles.includes('dmg');

  const buildArgs = removeFlagWithValue(forwarded, '--bundles')
    .filter((value) => value !== '--no-sign' && value !== '--skip-stapling');
  if (!buildArgs.includes('--no-bundle')) buildArgs.push('--no-bundle');

  // Tauri's Finder/AppleScript DMG helper has shown intermittent failures on
  // hosted ARM64 runners. Bundle the signed .app with Tauri, then create the
  // DMG deterministically with hdiutil in run-tauri.mjs.
  const bundleArgs = removeFlagWithValue(forwarded, '--bundles');
  bundleArgs[0] = 'bundle';
  bundleArgs.push('--bundles', 'app');
  const cleanBundleArgs = bundleArgs.filter((value) => value !== '--no-bundle');
  return { target, buildArgs, bundleArgs: cleanBundleArgs, wantsDmg };
}

export function signingIdentity({ project, forwarded }) {
  const requestedConfig = forwardedValue(forwarded, '--config');
  const candidates = [
    requestedConfig && resolve(project, requestedConfig),
    resolve(project, 'tauri.conf.json'),
  ].filter(Boolean);
  for (const path of candidates) {
    if (!existsSync(path)) continue;
    const config = JSON.parse(readFileSync(path, 'utf8'));
    const identity = config.bundle?.macOS?.signingIdentity;
    if (typeof identity === 'string' && identity.trim()) return identity.trim();
  }
  return '-';
}

export function prepareMacosBundledCli({
  root,
  project,
  targetDirectory,
  forwarded,
  environment,
  platform = process.platform,
  run = runChecked,
}) {
  const target = macosSetupBuild(forwarded, platform);
  if (!target) return null;

  run('cargo', [
    'build', '--release', '-p', 'fileflow-setup', '--bin', 'fileflow-setup-cli', '--target', target,
  ], { cwd: root, env: environment });

  const cli = resolve(targetDirectory, target, 'release', 'fileflow-setup-cli');
  if (!existsSync(cli)) throw new Error(`CLI Setup introuvable après compilation: ${cli}`);

  const identity = signingIdentity({ project, forwarded });
  const signArgs = ['--force', '--sign', identity];
  if (identity !== '-') signArgs.push('--timestamp', '--options', 'runtime');
  signArgs.push(cli);
  run('codesign', signArgs, { cwd: root, env: environment });
  run('codesign', ['--verify', '--strict', '--verbose=2', cli], { cwd: root, env: environment });
  console.log(`[setup-tauri] CLI embarqué pré-signé pour ${target} (${identity}).`);
  return cli;
}
