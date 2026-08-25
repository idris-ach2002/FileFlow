#!/usr/bin/env node
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyLiveUpdater } from './verify-live-updater.mjs';
import { verifyLiveDownloads } from './verify-live-downloads.mjs';
import {
  applicationBundleRoot,
  isDistributableArtifactName,
  selectWindowsSetupExecutable,
  setupBundleRoot,
  setupTargetRoot,
} from './artifact-layout.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const node = process.execPath;

const layoutTarget = 'aarch64-apple-darwin';
if (applicationBundleRoot(repo, layoutTarget) === setupBundleRoot(repo, layoutTarget)) {
  throw new Error('FileFlow and FileFlow Setup must never share a bundle root');
}
const customSetupRoot = resolve(repo, 'temporary-setup-target');
if (setupTargetRoot(repo, { FILEFLOW_SETUP_TARGET_DIR: customSetupRoot }) !== customSetupRoot) {
  throw new Error('FILEFLOW_SETUP_TARGET_DIR must control the isolated Setup target root');
}

if (isDistributableArtifactName('control.tar.gz') || isDistributableArtifactName('data.tar.gz')) {
  throw new Error('Debian package internals must never be collected as release assets');
}
for (const asset of [
  'FileFlow.app.tar.gz',
  'FileFlow.app.tar.gz.sig',
  'FileFlow_1.0.7_amd64.deb',
  'FileFlowSetup_1.0.7_amd64.AppImage',
  'FileFlowSetupCLI_x86_64-pc-windows-msvc.exe',
]) {
  if (!isDistributableArtifactName(asset)) throw new Error(`expected release artifact: ${asset}`);
}
const selectedWindowsSetup = selectWindowsSetupExecutable([
  'C:\\FileFlowSetup\\fileflow-setup-cli.exe',
  'C:\\FileFlowSetup\\Uninstall FileFlowSetup.exe',
  'C:\\FileFlowSetup\\fileflow-setup.exe',
]);
if (!selectedWindowsSetup?.toLowerCase().endsWith('fileflow-setup.exe')) {
  throw new Error(`Windows Setup smoke must select the GUI executable, got ${selectedWindowsSetup}`);
}

function run(script, args) {
  const result = spawnSync(node, [resolve(repo, script), ...args], { cwd: repo, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
function put(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, value);
}

const collectorTarget = `collector-self-test-${process.pid}`;
const collectorApplicationBundle = applicationBundleRoot(repo, collectorTarget);
const collectorSetupBundle = setupBundleRoot(repo, collectorTarget);
const collectorOutput = resolve(repo, 'dist', 'release', collectorTarget);
try {
  put(join(collectorApplicationBundle, 'deb', 'control.tar.gz'), 'application-control');
  put(join(collectorApplicationBundle, 'deb', 'data.tar.gz'), 'application-data');
  put(join(collectorApplicationBundle, 'deb', 'FileFlow_1.0.7_amd64.deb'), 'application-deb');
  put(join(collectorSetupBundle, 'deb', 'control.tar.gz'), 'setup-control');
  put(join(collectorSetupBundle, 'deb', 'data.tar.gz'), 'setup-data');
  put(join(collectorSetupBundle, 'deb', 'FileFlowSetup_1.0.7_amd64.deb'), 'setup-deb');
  put(join(collectorSetupBundle, 'setup-cli', 'FileFlowSetupCLI_x86_64-unknown-linux-gnu.bin'), 'setup-cli');
  run('scripts/release/collect-artifacts.mjs', ['--target', collectorTarget, '--include-setup']);
  const collected = readdirSync(collectorOutput).sort();
  if (collected.includes('control.tar.gz') || collected.includes('data.tar.gz')) {
    throw new Error(`Debian internals leaked into release assets: ${collected.join(', ')}`);
  }
  for (const expected of [
    'FileFlow_1.0.7_amd64.deb',
    'FileFlowSetup_1.0.7_amd64.deb',
    'FileFlowSetupCLI_x86_64-unknown-linux-gnu.bin',
  ]) {
    if (!collected.includes(expected)) throw new Error(`collector omitted ${expected}`);
  }
} finally {
  rmSync(resolve(repo, 'target', collectorTarget), { recursive: true, force: true });
  rmSync(resolve(repo, 'target', 'fileflow-setup', collectorTarget), { recursive: true, force: true });
  rmSync(collectorOutput, { recursive: true, force: true });
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
  put(join(macArm, 'FileFlowSetup_1.0.2_aarch64.dmg'), 'setup-dmg-arm');
  put(join(macArm, 'FileFlowSetupCLI_aarch64-apple-darwin.bin'), 'setup-cli-arm');
  put(join(macArm, 'FileFlow.app.tar.gz'), 'updater-arm');
  put(join(macArm, 'FileFlow.app.tar.gz.sig'), 'signature-arm');
  put(join(macIntel, 'FileFlow_1.0.2_x64.dmg'), 'dmg-intel');
  put(join(macIntel, 'FileFlowSetup_1.0.2_x64.dmg'), 'setup-dmg-intel');
  put(join(macIntel, 'FileFlowSetupCLI_x86_64-apple-darwin.bin'), 'setup-cli-intel');
  put(join(macIntel, 'FileFlow.app.tar.gz'), 'updater-intel');
  put(join(macIntel, 'FileFlow.app.tar.gz.sig'), 'signature-intel');

  put(join(windows, 'FileFlow_1.0.2_x64.msi'), 'msi');
  put(join(windows, 'FileFlow_1.0.2_x64.msi.sig'), 'msi-signature');
  put(join(windows, 'FileFlow_1.0.2_x64-setup.exe'), 'nsis');
  put(join(windows, 'FileFlow_1.0.2_x64-setup.exe.sig'), 'nsis-signature');
  put(join(windows, 'FileFlowSetup_1.0.2_x64-setup.exe'), 'setup-nsis');
  put(join(windows, 'FileFlowSetupCLI_x86_64-pc-windows-msvc.exe'), 'setup-cli-windows');

  for (const [dir, arch] of [[linuxX64, 'amd64'], [linuxArm, 'arm64']]) {
    put(join(dir, `FileFlow_1.0.2_${arch}.deb`), `deb-${arch}`);
    put(join(dir, `FileFlow_1.0.2_${arch}.rpm`), `rpm-${arch}`);
    put(join(dir, 'FileFlow.AppImage'), `appimage-${arch}`);
    put(join(dir, 'FileFlow.AppImage.sig'), `appimage-signature-${arch}`);
    put(join(dir, `FileFlowSetup_${arch}.AppImage`), `setup-appimage-${arch}`);
    put(join(dir, `FileFlowSetupCLI_${arch}.bin`), `setup-cli-${arch}`);
  }

  const latest = join(temp, 'latest.json');
  const checksums = join(temp, 'SHA256SUMS');
  const downloads = join(temp, 'downloads.json');
  run('scripts/release/normalize-artifacts.mjs', ['--root', root]);
  run('scripts/release/generate-updater-manifest.mjs', [
    '--root', root,
    '--version', '1.0.2',
    '--repository', 'fileflow/self-test',
    '--output', latest,
  ]);
  run('scripts/release/generate-download-manifest.mjs', [
    '--root', root,
    '--version', '1.0.2',
    '--repository', 'fileflow/self-test',
    '--output', downloads,
  ]);
  run('scripts/release/generate-checksums.mjs', ['--root', root, '--output', checksums]);
  run('scripts/release/verify-release.mjs', ['--root', root, '--latest', latest, '--checksums', checksums]);
  run('scripts/release/verify-updater-transition.mjs', ['--from', '1.0.1', '--to', '1.0.2', '--manifest', latest]);
  run('scripts/release/assert-version-consistency.mjs', [JSON.parse(readFileSync(join(repo, 'package.json'), 'utf8')).version]);
  run('scripts/release/assert-version-newer.mjs', ['1.0.2', '1.0.1']);
  const rejectedDowngrade = spawnSync(node, [
    resolve(repo, 'scripts/release/assert-version-newer.mjs'), '1.0.1', '1.0.2',
  ], { cwd: repo, encoding: 'utf8' });
  if (rejectedDowngrade.status === 0) throw new Error('automatic promotion must reject a downgrade');

  let liveManifest;
  server = createServer((request, response) => {
    if (request.url?.startsWith('/latest.json')) {
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify(liveManifest));
      return;
    }
    if (request.url?.startsWith('/downloads.json')) {
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify(liveDownloads));
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
  let liveDownloads = JSON.parse(readFileSync(downloads, 'utf8'));
  for (const [platform, item] of Object.entries(liveManifest.platforms)) {
    item.url = `http://127.0.0.1:${address.port}/artifacts/${platform}`;
    item.signature = `synthetic-signature-${platform}`;
  }
  await verifyLiveUpdater({
    endpoint: `http://127.0.0.1:${address.port}/latest.json`,
    expectedVersion: '1.0.2',
    requiredPlatforms: Object.keys(liveManifest.platforms),
  });
  for (const item of Object.values(liveDownloads.platforms)) {
    item.application.url = `http://127.0.0.1:${address.port}/artifacts/application`;
    item.setup.url = `http://127.0.0.1:${address.port}/artifacts/setup`;
  }
  await verifyLiveDownloads({
    endpoint: `http://127.0.0.1:${address.port}/downloads.json`,
    expectedVersion: '1.0.2',
    requiredPlatforms: Object.keys(liveDownloads.platforms),
  });

  const sumLines = readFileSync(checksums, 'utf8').trim().split(/\r?\n/);
  if (sumLines.some((line) => /[\\/]/.test(line.split('  ')[1] || ''))) {
    throw new Error('SHA256SUMS must use flat GitHub Release asset names');
  }
  console.log('[self-test] release tooling + updater + Setup download portal passed');
} finally {
  if (server) await new Promise((resolveClose) => server.close(resolveClose));
  rmSync(temp, { recursive: true, force: true });
}
