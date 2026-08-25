#!/usr/bin/env node
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { applicationBundleRoot, setupBundleRoot } from './artifact-layout.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}
const target = args.get('--target');
const includeSetup = process.argv.includes('--include-setup');
if (!target) {
  throw new Error('usage: collect-artifacts.mjs --target <target> [--include-setup]');
}

const applicationBundle = applicationBundleRoot(root, target);
if (!existsSync(applicationBundle)) {
  throw new Error(`missing application bundle root ${applicationBundle}`);
}
const bundleRoots = [applicationBundle];
if (includeSetup) {
  const setupBundle = setupBundleRoot(root, target);
  if (!existsSync(setupBundle)) {
    throw new Error(`missing Setup bundle root ${setupBundle}`);
  }
  bundleRoots.push(setupBundle);
}

const output = resolve(root, 'dist', 'release', target);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });

const allowed = (name) => /\.(dmg|msi|exe|deb|rpm|appimage|sig|gz|bin)$/i.test(name);
function walk(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

const files = bundleRoots.flatMap((bundleRoot) => walk(bundleRoot))
  .filter((path) => allowed(basename(path)));
if (!files.length) throw new Error(`no distributable artifacts found for ${target}`);

const seen = new Set();
let totalBytes = 0;
const maxBytesRaw = process.env.FILEFLOW_DISTRIBUTION_MAX_BYTES?.trim();
const maxBytes = maxBytesRaw ? Number(maxBytesRaw) : 0;
if (maxBytesRaw && (!Number.isFinite(maxBytes) || maxBytes <= 0)) {
  throw new Error('FILEFLOW_DISTRIBUTION_MAX_BYTES must be a positive integer');
}
for (const source of files) {
  const name = basename(source);
  if (seen.has(name)) throw new Error(`duplicate artifact basename for ${target}: ${name}`);
  seen.add(name);
  const bytes = statSync(source).size;
  totalBytes += bytes;
  if (maxBytes && bytes > maxBytes) {
    throw new Error(`artifact exceeds FILEFLOW_DISTRIBUTION_MAX_BYTES: ${name} (${bytes} > ${maxBytes})`);
  }
  cpSync(source, join(output, name));
  console.log(`[collect] ${name}: ${(bytes / 1024 / 1024).toFixed(1)} MiB`);
}
console.log(`[collect] ${files.length} artifact(s), total ${(totalBytes / 1024 / 1024).toFixed(1)} MiB -> ${output}`);
