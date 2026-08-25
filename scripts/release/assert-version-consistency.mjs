#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const expected = process.argv[2];
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(expected || '')) {
  throw new Error('usage: assert-version-consistency.mjs X.Y.Z');
}

for (const relative of [
  'package.json',
  'frontend/package.json',
  'website/package.json',
  'src-tauri/tauri.conf.json',
  'setup-tauri/tauri.conf.json',
]) {
  const actual = JSON.parse(readFileSync(resolve(root, relative), 'utf8')).version;
  if (actual !== expected) throw new Error(`${relative}: ${actual} != ${expected}`);
}

for (const [relative, pattern] of [
  ['Cargo.toml', /\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m],
  ['src-tauri/Cargo.toml', /^version\s*=\s*"([^"]+)"/m],
]) {
  const match = readFileSync(resolve(root, relative), 'utf8').match(pattern);
  if (match?.[1] !== expected) throw new Error(`${relative}: ${match?.[1] || 'absent'} != ${expected}`);
}

console.log(`[release] versions cohérentes : ${expected}`);
