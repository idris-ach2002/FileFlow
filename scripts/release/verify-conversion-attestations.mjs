#!/usr/bin/env node

import {
  createHash,
} from 'node:crypto';

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
  parseArgs(
    process.argv.slice(2),
  );

const root =
  resolve(
    required('--root'),
  );

const version =
  required('--version')
    .replace(/^v/, '');

const sourceSha =
  required('--sha');

const output =
  resolve(
    args.get('--output') ||
    'CONVERSION-ATTESTATIONS.json',
  );

const corpus =
  JSON.parse(
    readFileSync(
      resolve(
        'tests/fixtures/conversions/corpus.json',
      ),
      'utf8',
    ),
  );

const targets = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'x86_64-pc-windows-msvc',
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
];

if (
  !/^\d+\.\d+\.\d+$/.test(
    version,
  )
) {
  throw new Error(
    `invalid version: ${version}`,
  );
}

if (
  !/^[0-9a-f]{40}$/.test(
    sourceSha,
  )
) {
  throw new Error(
    `invalid source SHA: ${sourceSha}`,
  );
}

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
  git.stdout.trim() !== sourceSha
) {
  throw new Error(
    'publish checkout SHA differs ' +
    'from conversion attestation SHA',
  );
}

const combined = [];

for (
  const target of targets
) {

  const directory =
    join(
      root,
      target,
    );

  const path =
    join(
      directory,
      'conversion-attestation.json',
    );

  if (
    !existsSync(path)
  ) {
    throw new Error(
      `missing conversion attestation ` +
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
      `conversion gate status invalid ` +
      `for ${target}`,
    );
  }

  if (
    attestation.target !== target ||
    attestation.version !== version ||
    attestation.sourceSha !== sourceSha
  ) {
    throw new Error(
      `conversion provenance mismatch ` +
      `for ${target}`,
    );
  }

  if (
    attestation.failed !== 0
  ) {
    throw new Error(
      `${target} reports conversion failures`,
    );
  }

  if (
    !attestation.candidateArtifact ||
    !/^[0-9a-f]{64}$/.test(
      attestation
        .candidateArtifact
        .sha256 ||
      '',
    )
  ) {
    throw new Error(
      `${target} has invalid candidate ` +
      `artifact fingerprint`,
    );
  }

  /*
   * Garantie cruciale :
   *
   * l'artefact candidat lié à la certification doit être
   * présent byte-for-byte dans le set qui va être publié.
   */
  const releaseHashes =
    new Set(
      walk(directory)
        .filter(
          (file) =>
            basename(file) !==
            'conversion-attestation.json',
        )
        .map(sha256),
    );

  if (
    !releaseHashes.has(
      attestation
        .candidateArtifact
        .sha256,
    )
  ) {
    throw new Error(
      `${target}: exact candidate artifact ` +
      `exercised by conversion gate is not ` +
      `present in publish set`,
    );
  }

  const byId =
    new Map(
      (
        attestation.cases ||
        []
      ).map(
        (item) => [
          item.id,
          item,
        ],
      ),
    );

  for (
    const definition of
    corpus.cases
  ) {

    const result =
      byId.get(
        definition.id,
      );

    if (!result) {
      throw new Error(
        `${target}: missing conversion ` +
        `result ${definition.id}`,
      );
    }

    if (
      result.status === 'passed'
    ) {
      continue;
    }

    if (
      result.status === 'skipped' &&
      (
        definition.optionalOn ||
        []
      ).includes(target)
    ) {
      continue;
    }

    throw new Error(
      `${target}: ${definition.id} ` +
      `did not pass (${result.status})`,
    );
  }

  if (
    byId.size !==
    corpus.cases.length
  ) {
    throw new Error(
      `${target}: unexpected conversion ` +
      `case count ${byId.size}/` +
      `${corpus.cases.length}`,
    );
  }

  combined.push(
    attestation,
  );

  console.log(
    `[conversion-gate] ${target}: ` +
    `${attestation.passed}/` +
    `${attestation.total}, ` +
    `skipped=${attestation.skipped}, ` +
    `exact artifact SHA PASS`,
  );
}

writeFileSync(
  output,
  (
    JSON.stringify(
      {
        schemaVersion: 1,

        status: 'passed',

        sourceSha,

        version,

        corpusVersion:
          corpus.schemaVersion,

        targets:
          combined,

        verifiedAt:
          new Date().toISOString(),
      },
      null,
      2,
    ) + '\n'
  ),
);

console.log(
  `[conversion-gate] PASS ` +
  `5/5 targets -> ${output}`,
);

function parseArgs(values) {

  const map =
    new Map();

  for (
    let index = 0;
    index < values.length;
    index += 2
  ) {

    const key =
      values[index];

    const value =
      values[index + 1];

    if (
      !key?.startsWith('--') ||
      value == null
    ) {
      throw new Error(
        `invalid arguments near ` +
        `${key || '<end>'}`,
      );
    }

    map.set(
      key,
      value,
    );
  }

  return map;
}

function required(name) {

  const value =
    args.get(name);

  if (!value) {
    throw new Error(
      `missing ${name}`,
    );
  }

  return value;
}

function walk(directory) {

  if (!existsSync(directory)) {
    return [];
  }

  return readdirSync(
    directory,
    {
      withFileTypes: true,
    },
  ).flatMap(
    (entry) => {

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
    },
  );
}

function sha256(path) {

  return createHash('sha256')
    .update(
      readFileSync(path),
    )
    .digest('hex');
}
