#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = new Map();
for (let i = 2; i < process.argv.length; i += 2) argv.set(process.argv[i], process.argv[i + 1]);
const root = resolve(repo, argv.get('--root') || 'dist/release');
const output = resolve(repo, argv.get('--output') || 'SHA256SUMS');
if (!existsSync(root)) throw new Error(`checksum root does not exist: ${root}`);

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

const files = walk(root).filter((path) => resolve(path) !== output && basename(path) !== 'SHA256SUMS').sort();
if (!files.length) throw new Error('no files to checksum');
const names = new Set();
const lines = files.map((path) => {
  const name = basename(path);
  if (names.has(name)) throw new Error(`release assets must have globally unique basenames before checksums: ${name}`);
  names.add(name);
  const hash = createHash('sha256').update(readFileSync(path)).digest('hex');
  return `${hash}  ${name}`;
});
writeFileSync(output, `${lines.join('\n')}\n`);
console.log(`[checksums] ${files.length} flat release asset(s) -> ${output}`);
