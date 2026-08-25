#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) args.set(process.argv[index], process.argv[index + 1]);
const root = resolve(repoRoot, args.get('--root') || 'dist/release');
const version = args.get('--version');
const repository = args.get('--repository');
const output = resolve(repoRoot, args.get('--output') || 'downloads.json');
if (!version || !repository) {
  throw new Error('usage: generate-download-manifest.mjs --root dist/release --version X.Y.Z --repository owner/repo [--output downloads.json]');
}
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) throw new Error(`invalid version: ${version}`);
if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) throw new Error(`invalid repository: ${repository}`);

const targets = new Map([
  ['aarch64-apple-darwin', { key: 'darwin-aarch64', type: 'dmg' }],
  ['x86_64-apple-darwin', { key: 'darwin-x86_64', type: 'dmg' }],
  ['x86_64-pc-windows-msvc', { key: 'windows-x86_64', type: 'exe', setupVariants: ['exe', 'msi'] }],
  ['x86_64-unknown-linux-gnu', { key: 'linux-x86_64', type: 'appimage', setupVariants: ['appimage', 'deb', 'rpm'] }],
  ['aarch64-unknown-linux-gnu', { key: 'linux-aarch64', type: 'appimage', setupVariants: ['appimage', 'deb', 'rpm'] }],
]);

function filesBelow(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesBelow(path) : [path];
  });
}

function matchesType(name, type) {
  if (type === 'dmg') return /\.dmg$/i.test(name);
  if (type === 'exe') return /\.exe$/i.test(name) && !/\.exe\.sig$/i.test(name);
  if (type === 'msi') return /\.msi$/i.test(name) && !/\.msi\.sig$/i.test(name);
  if (type === 'deb') return /\.deb$/i.test(name);
  if (type === 'rpm') return /\.rpm$/i.test(name);
  return /\.appimage$/i.test(name) && !/\.appimage\.sig$/i.test(name);
}

function pick(files, type, setup) {
  const candidates = files.filter((path) => {
    const name = basename(path);
    if (/fileflow[ _.-]?setup[ _.-]?cli/i.test(name)) return false;
    const isSetup = /fileflow[ _.-]?setup/i.test(name);
    if (isSetup !== setup) return false;
    return matchesType(name, type);
  });
  if (candidates.length !== 1) {
    throw new Error(`expected exactly one ${setup ? 'setup' : 'application'} ${type}, found ${candidates.map((path) => basename(path)).join(', ') || 'none'}`);
  }
  return candidates[0];
}

function artifact(path, packageType) {
  const name = basename(path);
  const bytes = readFileSync(path);
  const signaturePath = `${path}.sig`;
  return {
    name,
    url: `https://github.com/${repository}/releases/download/v${version}/${encodeURIComponent(name)}`,
    sha256: createHash('sha256').update(bytes).digest('hex'),
    size: statSync(path).size,
    signature: existsSync(signaturePath) ? readFileSync(signaturePath, 'utf8').trim() : null,
    packageType,
  };
}

const platforms = {};
for (const [target, descriptor] of targets) {
  const files = filesBelow(join(root, target));
  const applicationPath = pick(files, descriptor.type, false);
  const setupPath = pick(files, descriptor.type, true);
  const setupVariants = Object.fromEntries(
    [...new Set(descriptor.setupVariants || [descriptor.type])].map((type) => [
      type,
      artifact(pick(files, type, true), type),
    ]),
  );
  const cliPath = files.find((path) => /fileflow[ _.-]?setup[ _.-]?cli/i.test(basename(path)) && !/\.(sig|sha256)$/i.test(path));
  platforms[descriptor.key] = {
    application: artifact(applicationPath, descriptor.type),
    setup: artifact(setupPath, descriptor.type),
    setupVariants,
    ...(cliPath ? { cli: artifact(cliPath, 'binary') } : {}),
  };
}

const manifest = {
  schemaVersion: 1,
  version,
  publishedAt: new Date().toISOString(),
  repository,
  platforms,
};
writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`[downloads] ${Object.keys(platforms).length} platforms -> ${output}`);
