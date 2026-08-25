#!/usr/bin/env node
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { findLocalApplication, targetTriple } from './local-source.mjs';

assert.equal(targetTriple('darwin', 'arm64'), 'aarch64-apple-darwin');
assert.equal(targetTriple('darwin', 'x64'), 'x86_64-apple-darwin');
assert.equal(targetTriple('linux', 'arm64'), 'aarch64-unknown-linux-gnu');

const root = mkdtempSync(join(tmpdir(), 'fileflow-local-source-'));
try {
  const bundle = join(root, 'target/aarch64-apple-darwin/release/bundle/dmg');
  mkdirSync(bundle, { recursive: true });
  writeFileSync(join(bundle, 'FileFlow_1.0.5_aarch64.dmg'), 'old');
  writeFileSync(join(bundle, 'FileFlowSetup_1.0.6_aarch64.dmg'), 'setup');
  const expected = join(bundle, 'FileFlow_1.0.6_aarch64.dmg');
  writeFileSync(expected, 'application');
  assert.equal(findLocalApplication({
    root,
    version: '1.0.6',
    platform: 'darwin',
    architecture: 'arm64',
  }), expected);
  assert.equal(findLocalApplication({
    root,
    version: '2.0.0',
    platform: 'darwin',
    architecture: 'arm64',
  }), null);
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log('[setup-launcher] résolution locale versionnée vérifiée');
