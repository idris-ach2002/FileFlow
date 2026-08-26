#!/usr/bin/env node

import assert from 'node:assert/strict';

import {
  readFileSync,
} from 'node:fs';

const read =
  (path) =>
    readFileSync(
      path,
      'utf8',
    );

const corpus =
  JSON.parse(
    read(
      'tests/fixtures/conversions/corpus.json',
    ),
  );

assert.ok(
  Array.isArray(
    corpus.cases,
  ) &&
  corpus.cases.length >= 18,

  'conversion corpus must contain >=18 real cases',
);

const ids =
  new Set();

for (
  const item of corpus.cases
) {

  assert.match(
    item.id,
    /^[a-z0-9-]+$/,
  );

  assert.ok(
    !ids.has(item.id),
    `duplicate case ${item.id}`,
  );

  ids.add(item.id);

  assert.ok(
    Number.isInteger(
      item.budgetMs,
    ) &&
    item.budgetMs >= 5_000 &&
    item.budgetMs <= 120_000,

    `invalid budget ${item.id}`,
  );
}

for (
  const [
    label,
    file,
  ] of [
    [
      'Windows',
      '.github/workflows/release-windows.yml',
    ],
    [
      'macOS',
      '.github/workflows/release-macos.yml',
    ],
    [
      'Linux',
      '.github/workflows/release-linux.yml',
    ],
  ]
) {

  const source =
    read(file);

  assert.match(
    source,

    /cargo test -p fileflow-executor --all-features --locked/,

    `${label}: native executor tests missing`,
  );

  assert.ok(
    source.includes(
      'scripts/release/conversion-e2e.mjs',
    ),

    `${label}: conversion E2E missing`,
  );

  assert.ok(
    source.includes(
      'conversion-attestation.json',
    ),

    `${label}: attestation copy missing`,
  );

  assert.ok(
    !(
      /conversion-e2e[\s\S]{0,200}continue-on-error:\s*true/
    ).test(source),

    `${label}: gate may not continue on error`,
  );

  const smoke =
    source.indexOf(
      'smoke-packaged-setup.mjs',
    );

  const conversion =
    source.indexOf(
      'conversion-e2e.mjs',
    );

  const collect =
    source.indexOf(
      'collect-artifacts.mjs',
    );

  assert.ok(
    smoke >= 0 &&
    conversion > smoke &&
    collect > conversion,

    (
      `${label}: required order ` +
      `smoke -> conversions -> collect`
    ),
  );
}

const atomic =
  read(
    '.github/workflows/fileflow-release.yml',
  );

assert.ok(
  atomic.includes(
    'verify-conversion-attestations.mjs',
  ),

  'Atomic Release must verify conversion attestations',
);

assert.ok(
  atomic.includes(
    'CONVERSION-ATTESTATIONS.json',
  ),

  'combined conversion attestation must be a release asset',
);

const verify =
  atomic.indexOf(
    'verify-conversion-attestations.mjs',
  );

const manifest =
  atomic.indexOf(
    'generate-updater-manifest.mjs',
  );

const publish =
  atomic.indexOf(
    'gh release create',
  );

assert.ok(
  verify >= 0 &&
  manifest > verify &&
  publish > verify,

  (
    'conversion verification must precede ' +
    'manifests/publication'
  ),
);

const gate =
  read(
    'scripts/release/conversion-e2e.mjs',
  );

assert.match(
  gate,
  /run\(\s*setupCli,\s*\[\s*'engines',/m,
  'conversion gate must install engines through packaged Setup CLI',
);

assert.match(
  gate,
  /run\(\s*setupCli,\s*\[\s*'doctor',/m,
  'conversion gate must run doctor through packaged Setup CLI',
);

for (
  const token of [
    'image-convert-webp',
    'images-to-pdf',
    'office-to-pdf',
    'pdf-merge',
    'pdf-to-images',
    'media-compatible',
    'audio-convert',
    'extract-audio',
    'video-to-gif',
    'archive-zip-roundtrip',
    'tar-zst-roundtrip',
    'tar-lz4-roundtrip',
    'html-to-pdf',
    'invalid-image-rejected',
    'candidateArtifact',
    'sourceSha',
  ]
) {

  assert.ok(
    gate.includes(token),

    `conversion gate missing ${token}`,
  );
}

console.log(
  `[conversion-gate] static contract verified: ` +
  `${corpus.cases.length} real cases × ` +
  `5 native targets`,
);
