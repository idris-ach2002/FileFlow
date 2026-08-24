#!/usr/bin/env node
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyLiveUpdater } from './verify-live-updater.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const node = process.execPath;
function run(script, args) {
  const result = spawnSync(node, [resolve(repo, script), ...args], { cwd: repo, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
function put(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, value);
}

const temp = mkdtempSync(join(tmpdir(), 'fileflow-release-tooling-'));
const root = join(temp, 'release');
let server;
try {
  const macArm = join(root, 'aarch64-apple-darwin');
  const macIntel = join(root, 'x86_64-apple-darwin');
  const windows = join(root, 'x86_64-pc-windows-msvc');
  const linuxX64 = join(root, 'x86_64-unknown-linux-gnu');
  const linuxArm = join(root, 'aarch64-unknown-linux-gnu');

  put(join(macArm, 'FileFlow_1.0.2_aarch64.dmg'), 'dmg-arm');
  put(join(macArm, 'FileFlow.app.tar.gz'), 'updater-arm');
  put(join(macArm, 'FileFlow.app.tar.gz.sig'), 'signature-arm');
  put(join(macIntel, 'FileFlow_1.0.2_x64.dmg'), 'dmg-intel');
  put(join(macIntel, 'FileFlow.app.tar.gz'), 'updater-intel');
  put(join(macIntel, 'FileFlow.app.tar.gz.sig'), 'signature-intel');

  put(join(windows, 'FileFlow_1.0.2_x64.msi'), 'msi');
  put(join(windows, 'FileFlow_1.0.2_x64.msi.sig'), 'msi-signature');
  put(join(windows, 'FileFlow_1.0.2_x64-setup.exe'), 'nsis');
  put(join(windows, 'FileFlow_1.0.2_x64-setup.exe.sig'), 'nsis-signature');

  for (const [dir, arch] of [[linuxX64, 'amd64'], [linuxArm, 'arm64']]) {
    put(join(dir, `FileFlow_1.0.2_${arch}.deb`), `deb-${arch}`);
    put(join(dir, `FileFlow_1.0.2_${arch}.rpm`), `rpm-${arch}`);
    put(join(dir, 'FileFlow.AppImage'), `appimage-${arch}`);
    put(join(dir, 'FileFlow.AppImage.sig'), `appimage-signature-${arch}`);
  }

  const latest = join(temp, 'latest.json');
  const checksums = join(temp, 'SHA256SUMS');
  run('scripts/release/normalize-artifacts.mjs', ['--root', root]);
  run('scripts/release/generate-updater-manifest.mjs', [
    '--root', root,
    '--version', '1.0.2',
    '--repository', 'fileflow/self-test',
    '--output', latest,
  ]);
  run('scripts/release/generate-checksums.mjs', ['--root', root, '--output', checksums]);
  run('scripts/release/verify-release.mjs', ['--root', root, '--latest', latest, '--checksums', checksums]);
  run('scripts/release/verify-updater-transition.mjs', ['--from', '1.0.1', '--to', '1.0.2', '--manifest', latest]);

  let liveManifest;
  server = createServer((request, response) => {
    if (request.url?.startsWith('/latest.json')) {
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify(liveManifest));
      return;
    }
    if (request.url?.startsWith('/artifacts/')) {
      response.writeHead(request.headers.range ? 206 : 200, {
        'Content-Type': 'application/octet-stream',
        'Content-Length': '1',
        ...(request.headers.range ? { 'Content-Range': 'bytes 0-0/1' } : {}),
      });
      response.end('x');
      return;
    }
    response.writeHead(404);
    response.end('Not Found');
  });
  await new Promise((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(0, '127.0.0.1', resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('unable to start updater self-test server');
  liveManifest = JSON.parse(readFileSync(latest, 'utf8'));
  for (const [platform, item] of Object.entries(liveManifest.platforms)) {
    item.url = `http://127.0.0.1:${address.port}/artifacts/${platform}`;
    item.signature = `synthetic-signature-${platform}`;
  }
  await verifyLiveUpdater({
    endpoint: `http://127.0.0.1:${address.port}/latest.json`,
    expectedVersion: '1.0.2',
    requiredPlatforms: Object.keys(liveManifest.platforms),
  });

  const sumLines = readFileSync(checksums, 'utf8').trim().split(/\r?\n/);
  if (sumLines.some((line) => /[\\/]/.test(line.split('  ')[1] || ''))) {
    throw new Error('SHA256SUMS must use flat GitHub Release asset names');
  }
  console.log('[self-test] release tooling + synthetic updater 1.0.1 -> 1.0.2 passed');
} finally {
  if (server) await new Promise((resolveClose) => server.close(resolveClose));
  rmSync(temp, { recursive: true, force: true });
}
