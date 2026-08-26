#!/usr/bin/env node

import { createHash } from 'node:crypto';

import {
  existsSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from 'node:fs';

import {
  basename,
  join,
  resolve,
} from 'node:path';

import {
  spawnSync,
} from 'node:child_process';

const args =
  new Map();

for (
  let index = 2;
  index < process.argv.length;
  index += 2
) {

  args.set(
    process.argv[index],
    process.argv[index + 1],
  );
}

const root =
  resolve(
    args.get('--root') ||
    'dist/release',
  );

const version =
  (
    args.get('--version') ||
    ''
  ).replace(/^v/, '');

const expectedSha =
  args.get('--sha') ||
  '';

const output =
  resolve(
    args.get('--output') ||
    'E2E-ATTESTATIONS.json',
  );

if (
  !/^\d+\.\d+\.\d+$/.test(
    version,
  ) ||
  !/^[0-9a-f]{40}$/.test(
    expectedSha,
  )
) {

  throw new Error(
    'usage: verify-e2e-attestations.mjs ' +
    '--root dist/release ' +
    '--version X.Y.Z ' +
    '--sha <40hex> ' +
    '--output E2E-ATTESTATIONS.json',
  );
}

const targets = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
];

const requiredScenarios = [
  'clean-install-exact-artifact',
  'installed-postcheck-launch',
  'repair-after-deliberate-damage',
  'packaged-cli-uninstall',
  'install-public-n-minus-1',
  'upgrade-n-minus-1-to-candidate',
  'post-upgrade-uninstall',
];

const git =
  spawnSync(
    'git',
    [
      'rev-parse',
      'HEAD',
    ],
    {
      encoding: 'utf8',
    },
  );

if (
  git.status !== 0 ||
  git.stdout.trim() !==
    expectedSha
) {

  throw new Error(
    'publish checkout SHA differs ' +
    'from E2E release SHA',
  );
}

const attestations = [];

for (const target of targets) {

  const directory =
    join(
      root,
      target,
    );

  const path =
    join(
      directory,
      'e2e-attestation.json',
    );

  if (!existsSync(path)) {

    throw new Error(
      `missing E2E attestation ` +
      `for ${target}`,
    );
  }

  const attestation =
    JSON.parse(
      readFileSync(
        path,
        'utf8',
      ),
    );

  if (
    attestation.schemaVersion !== 1 ||
    attestation.status !== 'passed'
  ) {

    throw new Error(
      `invalid E2E status ` +
      `for ${target}`,
    );
  }

  if (
    attestation.target !== target
  ) {

    throw new Error(
      `E2E target mismatch ` +
      `for ${target}`,
    );
  }

  if (
    attestation.version !== version
  ) {

    throw new Error(
      `E2E version mismatch ` +
      `for ${target}`,
    );
  }

  if (
    attestation.sourceSha !==
    expectedSha
  ) {

    throw new Error(
      `E2E source SHA mismatch ` +
      `for ${target}`,
    );
  }

  for (
    const scenario of
    requiredScenarios
  ) {

    if (
      !attestation
        .scenarios
        ?.includes(
          scenario,
        )
    ) {

      throw new Error(
        `${target} did not pass ` +
        `required scenario: ` +
        `${scenario}`,
      );
    }
  }

  if (
    !Array.isArray(
      attestation
        .testedArtifacts,
    ) ||
    attestation
      .testedArtifacts
      .length < 2
  ) {

    throw new Error(
      `${target} has no tested ` +
      `artifact fingerprint set`,
    );
  }

  const publishedHashes =
    new Set(
      walk(directory)

        .filter(
          (file) =>
            basename(file) !==
            'e2e-attestation.json',
        )

        .map(sha256),
    );

  for (
    const artifact of
    attestation.testedArtifacts
  ) {

    if (
      !/^[0-9a-f]{64}$/.test(
        artifact.sha256 ||
        '',
      )
    ) {

      throw new Error(
        `${target} has invalid ` +
        `artifact SHA`,
      );
    }

    if (
      !publishedHashes.has(
        artifact.sha256,
      )
    ) {

      throw new Error(
        `${target}: tested artifact ` +
        `is not present byte-for-byte ` +
        `in publish set: ` +
        `${artifact.role} ` +
        `${artifact.name}`,
      );
    }
  }

  attestations.push(
    attestation,
  );

  console.log(
    `[e2e-gate] ${target}: ` +
    `exact SHA + install/launch/` +
    `repair/upgrade/uninstall PASS`,
  );
}

writeFileSync(
  output,

  `${JSON.stringify(
    {
      schemaVersion: 1,

      status: 'passed',

      sourceSha:
        expectedSha,

      version,

      targets:
        attestations,

      verifiedAt:
        new Date().toISOString(),
    },
    null,
    2,
  )}\n`,
);

console.log(
  `[e2e-gate] PASS 5/5 ` +
  `targets -> ${output}`,
);

function walk(directory) {

  if (!existsSync(directory)) {
    return [];
  }

  return readdirSync(
    directory,
    {
      withFileTypes: true,
    },
  ).flatMap((entry) => {

    const path =
      join(
        directory,
        entry.name,
      );

    return (
      entry.isDirectory()
        ? walk(path)
        : [path]
    );
  });
}

function sha256(path) {

  return createHash('sha256')
    .update(
      readFileSync(path),
    )
    .digest('hex');
}
