#!/usr/bin/env node

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
} from 'node:fs';
import { basename, join, resolve } from 'node:path';

const args = new Map();

for (let i = 2; i < process.argv.length; i += 2) {
  args.set(process.argv[i], process.argv[i + 1]);
}

const platform = args.get('--platform');
const root = resolve(args.get('--root') || 'dist/release');

if (!platform || !['macos', 'linux', 'windows'].includes(platform)) {
  throw new Error(
    'usage: canonicalize-installers.mjs --platform <macos|linux|windows> [--root dist/release]',
  );
}

if (!existsSync(root)) {
  throw new Error(`missing release root: ${root}`);
}

const installers = join(root, 'installers');
rmSync(installers, { recursive: true, force: true });
mkdirSync(installers, { recursive: true });

function filesBelow(dir) {
  if (!existsSync(dir)) return [];

  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? filesBelow(path) : [path];
  });
}

function choose(target, predicate, label) {
  const dir = join(root, target);

  const matches = filesBelow(dir)
    .filter((path) => statSync(path).isFile())
    .filter((path) => predicate(basename(path)))
    .sort();

  if (matches.length !== 1) {
    throw new Error(
      `${label}: expected exactly one source below ${dir}, found ${matches.length}: ${matches.join(', ')}`,
    );
  }

  return matches[0];
}

function publish(source, name) {
  const destination = join(installers, name);
  copyFileSync(source, destination);

  console.log(
    `[installer] ${basename(source)} -> installers/${name}`,
  );
}

if (platform === 'macos') {
  publish(
    choose(
      'aarch64-apple-darwin',
      (name) => name.toLowerCase().endsWith('.dmg'),
      'macOS ARM64 DMG',
    ),
    'FileFlow-macOS-arm64.dmg',
  );

  publish(
    choose(
      'x86_64-apple-darwin',
      (name) => name.toLowerCase().endsWith('.dmg'),
      'macOS Intel DMG',
    ),
    'FileFlow-macOS-x64.dmg',
  );
}

if (platform === 'linux') {
  const targets = [
    ['x86_64-unknown-linux-gnu', 'x64'],
    ['aarch64-unknown-linux-gnu', 'arm64'],
  ];

  for (const [target, arch] of targets) {
    publish(
      choose(
        target,
        (name) => name.toLowerCase().endsWith('.appimage'),
        `Linux ${arch} AppImage`,
      ),
      `FileFlow-Linux-${arch}.AppImage`,
    );

    publish(
      choose(
        target,
        (name) => name.toLowerCase().endsWith('.deb'),
        `Linux ${arch} DEB`,
      ),
      `FileFlow-Linux-${arch}.deb`,
    );

    publish(
      choose(
        target,
        (name) => name.toLowerCase().endsWith('.rpm'),
        `Linux ${arch} RPM`,
      ),
      `FileFlow-Linux-${arch}.rpm`,
    );
  }
}

if (platform === 'windows') {
  const target = 'x86_64-pc-windows-msvc';

  publish(
    choose(
      target,
      (name) => name.toLowerCase().endsWith('.exe'),
      'Windows NSIS installer',
    ),
    'FileFlow-Windows-x64-Setup.exe',
  );

  publish(
    choose(
      target,
      (name) => name.toLowerCase().endsWith('.msi'),
      'Windows MSI installer',
    ),
    'FileFlow-Windows-x64.msi',
  );
}

console.log(`[installer] canonical ${platform} installers are ready`);
