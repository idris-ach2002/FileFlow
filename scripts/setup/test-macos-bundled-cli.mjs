#!/usr/bin/env node

import assert from 'node:assert/strict';
import { forwardedValue, macosBuildPipeline, macosSetupBuild, prepareMacosBundledCli, signingIdentity } from './macos-bundled-cli.mjs';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

assert.equal(
  forwardedValue(['build', '--target', 'x86_64-apple-darwin'], '--target'),
  'x86_64-apple-darwin',
);
assert.equal(
  macosSetupBuild(['build', '--target', 'x86_64-apple-darwin'], 'darwin'),
  'x86_64-apple-darwin',
);
assert.equal(
  macosSetupBuild(['build', '--target', 'aarch64-apple-darwin'], 'darwin'),
  'aarch64-apple-darwin',
);
assert.equal(macosSetupBuild(['dev', '--target', 'x86_64-apple-darwin'], 'darwin'), null);
assert.equal(macosSetupBuild(['build', '--target', 'x86_64-pc-windows-msvc'], 'darwin'), null);
assert.equal(macosSetupBuild(['build', '--target', 'x86_64-apple-darwin'], 'linux'), null);

assert.deepEqual(
  macosBuildPipeline([
    'build', '--target', 'x86_64-apple-darwin', '--bundles', 'app,dmg', '--config', 'tauri.release.conf.json',
  ], 'darwin'),
  {
    target: 'x86_64-apple-darwin',
    buildArgs: [
      'build', '--target', 'x86_64-apple-darwin', '--config', 'tauri.release.conf.json', '--no-bundle',
    ],
    bundleArgs: [
      'bundle', '--target', 'x86_64-apple-darwin', '--bundles', 'app,dmg', '--config', 'tauri.release.conf.json',
    ],
  },
);
assert.deepEqual(
  macosBuildPipeline([
    'build', '--target', 'aarch64-apple-darwin', '--bundles', 'app,dmg', '--config', 'tauri.release.conf.json', '--no-sign',
  ], 'darwin'),
  {
    target: 'aarch64-apple-darwin',
    buildArgs: [
      'build', '--target', 'aarch64-apple-darwin', '--config', 'tauri.release.conf.json', '--no-bundle',
    ],
    bundleArgs: [
      'bundle', '--target', 'aarch64-apple-darwin', '--bundles', 'app,dmg', '--config', 'tauri.release.conf.json', '--no-sign',
    ],
  },
);
assert.equal(macosBuildPipeline(['build', '--target', 'x86_64-apple-darwin'], 'linux'), null);

const project = mkdtempSync(join(tmpdir(), 'fileflow-setup-signing-'));
try {
  writeFileSync(join(project, 'tauri.conf.json'), JSON.stringify({ bundle: { macOS: { signingIdentity: '-' } } }));
  writeFileSync(join(project, 'release.json'), JSON.stringify({ bundle: { macOS: { signingIdentity: 'Developer ID Application: FileFlow' } } }));
  assert.equal(signingIdentity({ project, forwarded: ['build'] }), '-');
  assert.equal(
    signingIdentity({ project, forwarded: ['build', '--config', 'release.json'] }),
    'Developer ID Application: FileFlow',
  );
} finally {
  rmSync(project, { recursive: true, force: true });
}

const integrationRoot = mkdtempSync(join(tmpdir(), 'fileflow-setup-signing-flow-'));
try {
  const setupProject = join(integrationRoot, 'setup-tauri');
  const targetDirectory = join(integrationRoot, 'target/fileflow-setup');
  mkdirSync(setupProject, { recursive: true });
  writeFileSync(join(setupProject, 'tauri.conf.json'), JSON.stringify({ bundle: { macOS: { signingIdentity: '-' } } }));
  const calls = [];
  const cliPath = join(targetDirectory, 'x86_64-apple-darwin/release/fileflow-setup-cli');
  const run = (program, args, options) => {
    calls.push({ program, args: [...args], cwd: options.cwd });
    if (program === 'cargo') {
      mkdirSync(join(targetDirectory, 'x86_64-apple-darwin/release'), { recursive: true });
      writeFileSync(cliPath, 'fake executable');
    }
  };
  const prepared = prepareMacosBundledCli({
    root: integrationRoot,
    project: setupProject,
    targetDirectory,
    forwarded: ['build', '--target', 'x86_64-apple-darwin'],
    environment: {},
    platform: 'darwin',
    run,
  });
  assert.equal(prepared, cliPath);
  assert.deepEqual(calls.map((call) => call.program), ['cargo', 'codesign', 'codesign']);
  assert.deepEqual(calls[0].args.slice(-2), ['--target', 'x86_64-apple-darwin']);
  assert.deepEqual(calls[1].args.slice(0, 4), ['--force', '--sign', '-', cliPath]);
  assert.deepEqual(calls[2].args, ['--verify', '--strict', '--verbose=2', cliPath]);
} finally {
  rmSync(integrationRoot, { recursive: true, force: true });
}

console.log('[setup-macos-signing] target detection, identity selection and pre-sign flow verified');
