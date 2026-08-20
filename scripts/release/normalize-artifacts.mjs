#!/usr/bin/env node
import { existsSync, readdirSync, renameSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = new Map();
for (let i = 2; i < process.argv.length; i += 2) argv.set(process.argv[i], process.argv[i + 1]);
const root = resolve(repo, argv.get('--root') || 'dist/release');
if (!existsSync(root)) throw new Error(`missing artifact root: ${root}`);

const targets = readdirSync(root, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();

const groups = new Map();
for (const target of targets) {
  const dir = join(root, target);
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const path = join(dir, entry.name);
    const bucket = groups.get(entry.name) || [];
    bucket.push({ target, path });
    groups.set(entry.name, bucket);
  }
}

function splitCompoundExtension(name) {
  for (const suffix of ['.app.tar.gz.sig', '.app.tar.gz', '.AppImage.sig', '.AppImage', '.msi.sig', '.exe.sig', '.tar.gz.sha256', '.tar.gz']) {
    if (name.toLowerCase().endsWith(suffix.toLowerCase())) return [name.slice(0, -suffix.length), name.slice(-suffix.length)];
  }
  const extension = extname(name);
  return [name.slice(0, -extension.length), extension];
}

let renamed = 0;
for (const [name, entries] of groups) {
  if (entries.length < 2) continue;
  const [stem, extension] = splitCompoundExtension(name);
  for (const { target, path } of entries) {
    const next = join(dirname(path), `${stem}_${target}${extension}`);
    renameSync(path, next);
    console.log(`[normalize] ${basename(path)} -> ${basename(next)}`);
    renamed += 1;
  }
}
console.log(`[normalize] ${renamed} duplicate asset name(s) disambiguated`);
