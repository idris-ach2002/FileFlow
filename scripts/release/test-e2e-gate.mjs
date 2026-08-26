#!/usr/bin/env node

import assert from 'node:assert/strict';

import {
  readFileSync,
} from 'node:fs';

function source(path) {

  return readFileSync(
    path,
    'utf8',
  );
}

function before(
  text,
  left,
  right,
  label,
) {

  const a =
    text.indexOf(left);

  const b =
    text.indexOf(right);

  assert.ok(
    a >= 0,
    `${label}: missing ${left}`,
  );

  assert.ok(
    b >= 0,
    `${label}: missing ${right}`,
  );

  assert.ok(
    a < b,
    `${label}: ${left} ` +
    `must run before ${right}`,
  );
}

const windows =
  source(
    '.github/workflows/release-windows.yml',
  );

const macos =
  source(
    '.github/workflows/release-macos.yml',
  );

const linux =
  source(
    '.github/workflows/release-linux.yml',
  );

const atomic =
  source(
    '.github/workflows/fileflow-release.yml',
  );

const e2e =
  source(
    'scripts/release/e2e-native.mjs',
  );

const adapter =
  source(
    'setup-tauri/src/adapter.rs',
  );

/* Windows */

assert.match(
  windows,
  /e2e-native\.mjs --target x86_64-pc-windows-msvc/,
);

before(
  windows,
  'smoke-packaged-setup.mjs',
  'e2e-native.mjs',
  'Windows',
);

before(
  windows,
  'e2e-native.mjs',
  'collect-artifacts.mjs',
  'Windows',
);

/* macOS */

assert.match(
  macos,
  /e2e-native\.mjs --target '\$\{\{ matrix\.target \}\}'/,
);

before(
  macos,
  'smoke-packaged-setup.mjs',
  'e2e-native.mjs',
  'macOS',
);

before(
  macos,
  'e2e-native.mjs',
  'collect-artifacts.mjs',
  'macOS',
);

/* Linux */

assert.match(
  linux,
  /xvfb-run -a node scripts\/release\/e2e-native\.mjs --target '\$\{\{ matrix\.target \}\}'/,
);

before(
  linux,
  'smoke-packaged-setup.mjs',
  'e2e-native.mjs',
  'Linux',
);

before(
  linux,
  'e2e-native.mjs',
  'collect-artifacts.mjs',
  'Linux',
);

/* Native release hardening discovered by 1.0.9 Atomic Release */

for (
  const [label, workflow] of [
    ['Windows', windows],
    ['macOS', macos],
    ['Linux', linux],
  ]
) {
  assert.match(
    workflow,
    /GITHUB_TOKEN:\s*\$\{\{\s*github\.token\s*\}\}/,
    `${label}: native E2E must authenticate GitHub API requests`,
  );
}

assert.match(
  windows,
  /\$PSNativeCommandUseErrorActionPreference = \$true/,
  'Windows native commands must fail the workflow immediately',
);

assert.match(
  windows,
  /cargo clippy --locked -p fileflow-setup -p fileflow-setup-core --all-targets/,
  'Windows Setup regression clippy must not mutate Cargo.lock',
);

assert.ok(
  JSON.parse(source('package.json'))
    .scripts['setup:test']
    .includes('cargo test --locked -p fileflow-setup-core -p fileflow-setup'),
  'setup:test must run Cargo with --locked',
);

assert.match(
  e2e,
  /run\(\s*releaseSetupCli,\s*\[\s*'repair',\s*'--app-only',/m,
  'integration-only repair must not reinstall runtime engines',
);

assert.match(
  adapter,
  /command_exists\("pkexec"\)[\s\S]{0,260}var_os\("CI"\)\.is_none\(\)/,
  'Linux CI must not invoke interactive pkexec',
);

/* Atomic publication gate */

assert.match(
  atomic,
  /needs: \[linux, macos, windows\]/,
);

assert.match(
  atomic,
  /verify-e2e-attestations\.mjs/,
);

assert.match(
  atomic,
  /E2E-ATTESTATIONS\.json/,
);

assert.match(
  atomic,
  /validation_only:/,
);

assert.match(
  atomic,
  /inputs\.validation_only != true/,
);

before(
  atomic,
  'verify-e2e-attestations.mjs',
  'generate-updater-manifest.mjs',
  'Atomic release',
);

before(
  atomic,
  'verify-e2e-attestations.mjs',
  'gh release create',
  'Atomic release',
);

/*
 * Critical security invariant:
 *
 * candidate local-artifact injection MUST remain DEBUG ONLY.
 *
 * The published release Setup CLI must never allow this bypass.
 */

assert.match(
  adapter,
  /#\[cfg\(debug_assertions\)\][\s\S]{0,500}FILEFLOW_SETUP_LOCAL_APPLICATION/,
  'local artifact bypass must stay debug-only',
);

/* E2E semantic coverage */

for (
  const marker of [
    'FILEFLOW_SETUP_LOCAL_APPLICATION',
    'clean-install-exact-artifact',
    'repair-after-deliberate-damage',
    'upgrade-n-minus-1-to-candidate',
    'packaged-cli-uninstall',
    'windows-msi-install-uninstall',
    'linux-deb-install-uninstall',
    'linux-rpm-fedora-install-uninstall',
  ]
) {

  assert.ok(
    e2e.includes(marker),
    `E2E orchestrator missing ${marker}`,
  );
}

console.log(
  '[e2e-gate] static release gate contract verified',
);
