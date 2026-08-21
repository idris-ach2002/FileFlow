#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = new Map();
for (let i = 2; i < process.argv.length; i += 2) argv.set(process.argv[i], process.argv[i + 1]);
const root = resolve(repo, argv.get('--root') || 'dist/release');
const checksum = resolve(repo, argv.get('--checksums') || 'SHA256SUMS');
const latest = resolve(repo, argv.get('--latest') || 'latest.json');
const targets = {
  'aarch64-apple-darwin': [/\.dmg$/i, /\.app\.tar\.gz$/i, /\.app\.tar\.gz\.sig$/i],
  'x86_64-apple-darwin': [/\.dmg$/i, /\.app\.tar\.gz$/i, /\.app\.tar\.gz\.sig$/i],
  'x86_64-pc-windows-msvc': [/\.msi$/i, /-setup\.exe$/i, /\.msi\.sig$/i, /-setup\.exe\.sig$/i],
  'x86_64-unknown-linux-gnu': [/\.deb$/i, /\.rpm$/i, /\.appimage$/i, /\.appimage\.sig$/i],
  'aarch64-unknown-linux-gnu': [/\.deb$/i, /\.rpm$/i, /\.appimage$/i, /\.appimage\.sig$/i],
};

const assetByName = new Map();
for (const [target, patterns] of Object.entries(targets)) {
  const dir = join(root, target);
  if (!existsSync(dir)) throw new Error(`missing target artifact directory ${target}`);
  const names = readdirSync(dir).filter((name) => !name.startsWith('.'));
  for (const pattern of patterns) {
    if (!names.some((name) => pattern.test(name))) throw new Error(`${target} missing required artifact ${pattern}`);
  }
  for (const name of names) {
    if (assetByName.has(name)) throw new Error(`duplicate global release asset basename: ${name}`);
    assetByName.set(name, join(dir, name));
  }
}

if (!existsSync(latest)) throw new Error('latest.json missing');
const manifest = JSON.parse(readFileSync(latest, 'utf8'));
const expectedPlatforms = ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'linux-x86_64', 'linux-aarch64'];
if (Object.keys(manifest.platforms || {}).length !== expectedPlatforms.length) throw new Error('latest.json must contain exactly five desktop targets');
for (const key of expectedPlatforms) {
  const item = manifest.platforms?.[key];
  if (!item?.url || !item?.signature) throw new Error(`latest.json missing updater entry ${key}`);
  const assetName = decodeURIComponent(new URL(item.url).pathname.split('/').at(-1));
  const assetPath = assetByName.get(assetName);
  if (!assetPath) throw new Error(`latest.json references missing release asset ${assetName}`);
  const signaturePath = assetByName.get(`${assetName}.sig`);
  if (!signaturePath) throw new Error(`latest.json signature asset missing for ${assetName}`);
  const signature = readFileSync(signaturePath, 'utf8').trim();
  if (signature !== item.signature.trim()) throw new Error(`latest.json signature mismatch for ${assetName}`);
}

if (!existsSync(checksum)) throw new Error('SHA256SUMS missing');
const checksumNames = new Set();
for (const line of readFileSync(checksum, 'utf8').trim().split(/\r?\n/)) {
  const match = line.match(/^([0-9a-f]{64})  ([^/\\]+)$/);
  if (!match) throw new Error(`invalid flat checksum line ${line}`);
  const [, expected, name] = match;
  if (checksumNames.has(name)) throw new Error(`duplicate checksum entry ${name}`);
  checksumNames.add(name);
  const file = assetByName.get(name);
  if (!file) throw new Error(`checksum references missing release asset ${name}`);
  const actual = createHash('sha256').update(readFileSync(file)).digest('hex');
  if (actual !== expected) throw new Error(`checksum mismatch ${name}`);
}
if (checksumNames.size !== assetByName.size) {
  const missing = [...assetByName.keys()].filter((name) => !checksumNames.has(name));
  throw new Error(`SHA256SUMS does not cover every release asset: ${missing.join(', ')}`);
}
console.log('[release] atomic artifact set, updater metadata and flat SHA-256 integrity verified');
