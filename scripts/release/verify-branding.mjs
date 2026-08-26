#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { applicationBundleRoot, setupBundleRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const targetIndex = process.argv.indexOf('--target');
const target = targetIndex >= 0 ? process.argv[targetIndex + 1] : null;

function json(path) {
  return JSON.parse(readFileSync(resolve(root, path), 'utf8'));
}

function requireFile(path) {
  const absolute = resolve(root, path);
  if (!existsSync(absolute) || statSync(absolute).size <= 0) {
    throw new Error(`branding asset missing or empty: ${path}`);
  }
  return absolute;
}

function walk(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? [path, ...walk(path)] : [path];
  });
}

const canonical = [
  'src-tauri/icons/32x32.png',
  'src-tauri/icons/128x128.png',
  'src-tauri/icons/128x128@2x.png',
  'src-tauri/icons/icon.png',
  'src-tauri/icons/icon.icns',
  'src-tauri/icons/icon.ico',
];
canonical.forEach(requireFile);

const app = json('src-tauri/tauri.conf.json');
const setup = json('setup-tauri/tauri.conf.json');
const expectedApp = [
  'icons/32x32.png', 'icons/128x128.png', 'icons/icon.png',
  'icons/128x128@2x.png', 'icons/icon.icns', 'icons/icon.ico',
];
const expectedSetup = expectedApp.map((path) => `../src-tauri/${path}`);
for (const icon of expectedApp) {
  if (!app.bundle?.icon?.includes(icon)) throw new Error(`FileFlow bundle missing icon declaration: ${icon}`);
}
for (const icon of expectedSetup) {
  if (!setup.bundle?.icon?.includes(icon)) throw new Error(`FileFlow Setup bundle missing canonical icon declaration: ${icon}`);
}
for (const icon of [
  '../src-tauri/icons/32x32.png',
  '../src-tauri/icons/64x64.png',
  '../src-tauri/icons/128x128.png',
  '../src-tauri/icons/128x128@2x.png',
  '../src-tauri/icons/icon.png',
]) {
  if (!setup.bundle?.resources?.includes(icon)) {
    throw new Error(`FileFlow Setup must embed Linux integration icon resource: ${icon}`);
  }
}

const adapter = readFileSync(resolve(root, 'setup-tauri/src/adapter.rs'), 'utf8');
for (const marker of [
  'Icon=fileflow',
  'fileflow.png',
  `IconLocation=($Target + ',0')`,
  'icon_sources',
]) {
  if (!adapter.includes(marker)) throw new Error(`system integration missing branding invariant: ${marker}`);
}

if (target) {
  const appFiles = walk(applicationBundleRoot(root, target));
  const setupFiles = walk(setupBundleRoot(root, target));
  if (target.endsWith('apple-darwin')) {
    const appIcns = appFiles.find((path) => /FileFlow\.app\/Contents\/Resources\/.*\.icns$/i.test(path));
    const setupIcns = setupFiles.find((path) => /FileFlowSetup\.app\/Contents\/Resources\/.*\.icns$/i.test(path));
    if (!appIcns || !setupIcns) throw new Error(`macOS bundle icon missing for ${target}`);
  }
  if (target.includes('windows')) {
    if (!appFiles.some((path) => /fileflow.*\.(exe|msi)$/i.test(basename(path)))) {
      throw new Error(`Windows FileFlow installer missing for ${target}`);
    }
    if (!setupFiles.some((path) => /fileflow.*setup.*\.(exe|msi)$/i.test(basename(path)))) {
      throw new Error(`Windows FileFlow Setup installer missing for ${target}`);
    }
  }
}

console.log(`[branding] canonical FileFlow logo verified${target ? ` for ${target}` : ' for app + Setup configs'}`);
