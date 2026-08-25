import { existsSync, readdirSync, statSync } from 'node:fs';
import { resolve } from 'node:path';

export function findLocalApplication({ root, version, platform = process.platform, architecture = process.arch }) {
  const target = targetTriple(platform, architecture);
  if (!target) return null;
  const bundle = resolve(root, 'target', target, 'release', 'bundle');
  const extension = platform === 'darwin' ? '.dmg' : platform === 'win32' ? '.exe' : '.appimage';
  return walk(bundle)
    .filter((path) => path.toLowerCase().endsWith(extension))
    .filter((path) => !/fileflow[ _.-]?setup/i.test(path.split(/[\\/]/).pop() || ''))
    .filter((path) => (path.split(/[\\/]/).pop() || '').includes(version))
    .map((path) => ({ path, modified: statSync(path).mtimeMs }))
    .sort((left, right) => right.modified - left.modified)[0]?.path || null;
}

export function targetTriple(platform = process.platform, architecture = process.arch) {
  if (platform === 'darwin') {
    return architecture === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  }
  if (platform === 'win32') return 'x86_64-pc-windows-msvc';
  if (platform === 'linux') {
    return architecture === 'arm64' ? 'aarch64-unknown-linux-gnu' : 'x86_64-unknown-linux-gnu';
  }
  return null;
}

function walk(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}
