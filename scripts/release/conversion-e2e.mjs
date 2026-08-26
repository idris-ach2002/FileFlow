#!/usr/bin/env node

import { createHash } from 'node:crypto';

import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';

import {
  homedir,
  tmpdir,
} from 'node:os';

import {
  basename,
  dirname,
  extname,
  join,
  resolve,
} from 'node:path';

import {
  fileURLToPath,
  pathToFileURL,
} from 'node:url';

import {
  spawnSync,
} from 'node:child_process';

const repo =
  resolve(
    dirname(
      fileURLToPath(import.meta.url),
    ),
    '../..',
  );

const argv =
  parseArgs(
    process.argv.slice(2),
  );

const target =
  requiredArg('--target');

const version =
  requiredArg('--version')
    .replace(/^v/, '');

const outputArg =
  argv.get('--output');

if (
  !/^\d+\.\d+\.\d+$/.test(
    version,
  )
) {
  throw new Error(
    `invalid version: ${version}`,
  );
}

const TARGETS =
  new Map([
    [
      'aarch64-apple-darwin',
      {
        os: 'darwin',
        arch: 'arm64',
        primary: '.dmg',
      },
    ],
    [
      'x86_64-apple-darwin',
      {
        os: 'darwin',
        arch: 'x64',
        primary: '.dmg',
      },
    ],
    [
      'x86_64-pc-windows-msvc',
      {
        os: 'win32',
        arch: 'x64',
        primary: '.exe',
      },
    ],
    [
      'x86_64-unknown-linux-gnu',
      {
        os: 'linux',
        arch: 'x64',
        primary: '.appimage',
      },
    ],
    [
      'aarch64-unknown-linux-gnu',
      {
        os: 'linux',
        arch: 'arm64',
        primary: '.appimage',
      },
    ],
  ]);

const descriptor =
  TARGETS.get(target);

if (!descriptor) {
  throw new Error(
    `unsupported target: ${target}`,
  );
}

if (
  process.platform !== descriptor.os ||
  process.arch !== descriptor.arch
) {
  throw new Error(
    `native runner mismatch: got ` +
    `${process.platform}/${process.arch}, ` +
    `expected ${descriptor.os}/${descriptor.arch}`,
  );
}

const sourceSha =
  capture(
    'git',
    [
      'rev-parse',
      'HEAD',
    ],
  ).trim();

if (
  !/^[0-9a-f]{40}$/.test(
    sourceSha,
  )
) {
  throw new Error(
    `invalid source SHA: ${sourceSha}`,
  );
}

const corpusPath =
  resolve(
    repo,
    'tests/fixtures/conversions/corpus.json',
  );

const corpus =
  JSON.parse(
    readFileSync(
      corpusPath,
      'utf8',
    ),
  );

if (
  !Array.isArray(corpus.cases) ||
  corpus.cases.length < 18
) {
  throw new Error(
    'conversion corpus must contain at least 18 cases',
  );
}

/*
 * A case declared as a stable FileFlow action must really
 * exist in the production executor at this exact SHA.
 */
const executorSource =
  readFileSync(
    resolve(
      repo,
      'crates/fileflow-executor/src/lib.rs',
    ),
    'utf8',
  );

for (
  const item of corpus.cases
) {
  if (
    item.actionId &&
    !executorSource.includes(
      `"${item.actionId}"`,
    )
  ) {
    throw new Error(
      `stable conversion action missing ` +
      `from executor contract: ${item.actionId}`,
    );
  }
}

const mainBundle =
  resolve(
    repo,
    'target',
    target,
    'release',
    'bundle',
  );

const setupBundle =
  resolve(
    repo,
    'target',
    'fileflow-setup',
    target,
    'release',
    'bundle',
  );

const candidateArtifact =
  selectCandidateArtifact(
    mainBundle,
    descriptor.primary,
  );

const setupCli =
  selectSetupCli(
    setupBundle,
  );

makeExecutable(setupCli);

console.log(
  `[conversion-e2e] target=${target}`,
);

console.log(
  `[conversion-e2e] source=${sourceSha}`,
);

console.log(
  `[conversion-e2e] version=${version}`,
);

console.log(
  `[conversion-e2e] candidate=${candidateArtifact}`,
);

console.log(
  `[conversion-e2e] setup-cli=${setupCli}`,
);

/*
 * IMPORTANT:
 *
 * Ce n'est pas un runtime préparé artificiellement par la CI.
 *
 * Le Setup FileFlow de la release installe/vérifie lui-même
 * les moteurs qui seront utilisés par les conversions.
 */
const linuxArmEngineIds = [
  'ffmpeg',
  'vips',
  'imagemagick',
  'qpdf',
  'img2pdf',
  'poppler',
  'ghostscript',
  'tesseract',
  'ocrmypdf',
  'libreoffice',
  'pandoc',
  'exiftool',
  'sevenzip',
  'zstd',
  'lz4',
];

if (
  target ===
  'aarch64-unknown-linux-gnu'
) {
  console.log(
    '[conversion-e2e] Linux ARM64: ' +
    'Chromium excluded from Setup engine ' +
    'postcheck; html-to-pdf is optional ' +
    'on this native target.',
  );
}

run(
  setupCli,
  [
    'engines',
    '--yes',
    '--no-launch',
    ...(
      target ===
      'aarch64-unknown-linux-gnu'
        ? [
            '--engines',
            linuxArmEngineIds.join(','),
          ]
        : []
    ),
  ],
  {
    timeoutMs: 45 * 60_000,
  },
);

run(
  setupCli,
  [
    'doctor',
    '--yes',
    '--no-launch',
  ],
  {
    timeoutMs: 10 * 60_000,
  },
);

if (
  process.platform === 'win32'
) {
  refreshWindowsPath();
}

const engines =
  resolveEngines();

const requiredEngineIds = [
  'vips',
  'vipsheader',
  'img2pdf',
  'qpdf',
  'pdfinfo',
  'pdftoppm',
  'pdftotext',
  'office',
  'exiftool',
  'ffmpeg',
  'ffprobe',
  'archive',
  'pandoc',
  'zstd',
  'lz4',
];

for (
  const id of requiredEngineIds
) {
  if (!engines[id]) {
    throw new Error(
      `required native conversion engine ` +
      `missing after Setup: ${id}`,
    );
  }
}

const workRoot =
  resolve(
    tmpdir(),
    (
      `FileFlow conversion gate é space ` +
      `${target} ${Date.now()}`
    ),
  );

const fixturesRoot =
  resolve(
    repo,
    'tests/fixtures/conversions/sources',
  );

const fixtureCopy =
  join(
    workRoot,
    'fixtures é space',
  );

const outputs =
  join(
    workRoot,
    'outputs é space',
  );

mkdirSync(
  outputs,
  {
    recursive: true,
  },
);

cpSync(
  fixturesRoot,
  fixtureCopy,
  {
    recursive: true,
  },
);

const results = [];

let prepared = {};

try {

  prepareFixtures();

  // ==========================================================
  // IMAGE
  // ==========================================================

  await gateCase(
    'image-convert-webp',
    () => {

      const output =
        join(
          outputs,
          'photo convertie é.webp',
        );

      run(
        engines.vips,
        [
          'copy',
          prepared.png1,
          output,
        ],
      );

      assertWebp(output);

      assertImageDimensions(
        output,
        96,
        64,
      );
    },
  );

  await gateCase(
    'image-resize',
    () => {

      const output =
        join(
          outputs,
          'photo redimensionnée é.png',
        );

      run(
        engines.vips,
        [
          'thumbnail',
          prepared.png1,
          output,
          '48',
          '--height',
          '32',
        ],
      );

      assertPng(output);

      const dims =
        imageDimensions(output);

      if (
        dims.width > 48 ||
        dims.height > 32 ||
        dims.width < 1 ||
        dims.height < 1
      ) {
        throw new Error(
          `unexpected resized dimensions ` +
          `${dims.width}x${dims.height}`,
        );
      }
    },
  );

  // ==========================================================
  // IMAGE -> PDF
  // ==========================================================

  await gateCase(
    'images-to-pdf',
    () => {

      const output =
        join(
          outputs,
          'images réunies é.pdf',
        );

      run(
        engines.img2pdf,
        [
          prepared.png1,
          prepared.png2,
          '-o',
          output,
        ],
      );

      assertPdf(
        output,
        2,
      );

      prepared.imagesPdf =
        output;
    },
  );

  // ==========================================================
  // OFFICE -> PDF
  // ==========================================================

  await gateCase(
    'office-to-pdf',
    () => {

      const outDir =
        join(
          outputs,
          'office é',
        );

      mkdirSync(
        outDir,
        {
          recursive: true,
        },
      );

      run(
        engines.office,
        [
          '--headless',
          '--convert-to',
          'pdf',
          '--outdir',
          outDir,
          prepared.rtf,
        ],
        {
          timeoutMs: 60_000,
        },
      );

      const output =
        join(
          outDir,
          (
            basename(
              prepared.rtf,
              extname(prepared.rtf),
            ) + '.pdf'
          ),
        );

      assertPdf(
        output,
        1,
      );

      assertPdfText(
        output,
        'FILEFLOW_OFFICE_GATE',
      );

      prepared.officePdf =
        output;
    },
  );

  // ==========================================================
  // PDF
  // ==========================================================

  await gateCase(
    'pdf-merge',
    () => {

      const output =
        join(
          outputs,
          'pdf fusionné é.pdf',
        );

      run(
        engines.qpdf,
        [
          '--empty',
          '--pages',
          prepared.imagesPdf,
          prepared.officePdf,
          '--',
          output,
        ],
      );

      assertPdf(
        output,
        3,
      );

      prepared.mergedPdf =
        output;
    },
  );

  await gateCase(
    'pdf-compress',
    () => {

      const output =
        join(
          outputs,
          'pdf compressé é.pdf',
        );

      run(
        engines.qpdf,
        [
          '--stream-data=compress',
          '--object-streams=generate',
          prepared.mergedPdf,
          output,
        ],
      );

      assertPdf(
        output,
        3,
      );
    },
  );

  await gateCase(
    'pdf-to-images',
    () => {

      const prefix =
        join(
          outputs,
          'page extraite é',
        );

      run(
        engines.pdftoppm,
        [
          '-f',
          '1',
          '-singlefile',
          '-png',
          prepared.mergedPdf,
          prefix,
        ],
      );

      const output =
        `${prefix}.png`;

      assertPng(output);

      const dims =
        imageDimensions(output);

      if (
        dims.width < 10 ||
        dims.height < 10
      ) {
        throw new Error(
          'PDF raster output is unexpectedly tiny',
        );
      }
    },
  );

  // ==========================================================
  // METADATA
  // ==========================================================

  await gateCase(
    'strip-metadata',
    () => {

      const output =
        join(
          outputs,
          'metadata é.jpg',
        );

      copyFileSync(
        prepared.jpeg,
        output,
      );

      run(
        engines.exiftool,
        [
          '-overwrite_original',
          '-Artist=FileFlow Gate',
          output,
        ],
      );

      const before =
        capture(
          engines.exiftool,
          [
            '-s3',
            '-Artist',
            output,
          ],
        ).trim();

      if (
        before !== 'FileFlow Gate'
      ) {
        throw new Error(
          `metadata write failed: ${before}`,
        );
      }

      run(
        engines.exiftool,
        [
          '-overwrite_original',
          '-all=',
          output,
        ],
      );

      const after =
        capture(
          engines.exiftool,
          [
            '-s3',
            '-Artist',
            output,
          ],
        ).trim();

      if (after) {
        throw new Error(
          `metadata was not stripped: ${after}`,
        );
      }
    },
  );

  // ==========================================================
  // PANDOC
  // ==========================================================

  await gateCase(
    'pandoc-text',
    () => {

      const output =
        join(
          outputs,
          'pandoc é.txt',
        );

      run(
        engines.pandoc,
        [
          prepared.markdown,
          '-t',
          'plain',
          '-o',
          output,
        ],
      );

      const text =
        readFileSync(
          output,
          'utf8',
        );

      if (
        !text.includes(
          'FILEFLOW_PANDOC_GATE',
        )
      ) {
        throw new Error(
          'Pandoc semantic marker missing',
        );
      }
    },
  );

  // ==========================================================
  // VIDEO / AUDIO
  // ==========================================================

  await gateCase(
    'media-compatible',
    () => {

      const output =
        join(
          outputs,
          'vidéo compatible é.mp4',
        );

      run(
        engines.ffmpeg,
        [
          '-hide_banner',
          '-loglevel',
          'error',
          '-y',
          '-i',
          prepared.video,
          '-c:v',
          'mpeg4',
          '-q:v',
          '5',
          '-c:a',
          'aac',
          output,
        ],
        {
          timeoutMs: 60_000,
        },
      );

      const info =
        ffprobe(output);

      requireStream(
        info,
        'video',
      );

      requireStream(
        info,
        'audio',
      );

      requireDuration(
        info,
        0.5,
        2.5,
      );
    },
  );

  await gateCase(
    'audio-convert',
    () => {

      const output =
        join(
          outputs,
          'audio converti é.flac',
        );

      run(
        engines.ffmpeg,
        [
          '-hide_banner',
          '-loglevel',
          'error',
          '-y',
          '-i',
          prepared.wav,
          '-c:a',
          'flac',
          output,
        ],
      );

      const info =
        ffprobe(output);

      requireStream(
        info,
        'audio',
      );

      requireDuration(
        info,
        0.5,
        2.5,
      );

      if (
        !readFileSync(output)
          .subarray(0, 4)
          .equals(
            Buffer.from('fLaC'),
          )
      ) {
        throw new Error(
          'FLAC magic bytes missing',
        );
      }
    },
  );

  await gateCase(
    'extract-audio',
    () => {

      const output =
        join(
          outputs,
          'audio extrait é.wav',
        );

      run(
        engines.ffmpeg,
        [
          '-hide_banner',
          '-loglevel',
          'error',
          '-y',
          '-i',
          prepared.video,
          '-vn',
          '-c:a',
          'pcm_s16le',
          output,
        ],
      );

      const info =
        ffprobe(output);

      requireStream(
        info,
        'audio',
      );

      if (
        info.streams.some(
          (stream) =>
            stream.codec_type ===
            'video',
        )
      ) {
        throw new Error(
          'video stream unexpectedly present ' +
          'in extracted audio',
        );
      }
    },
  );

  await gateCase(
    'video-to-gif',
    () => {

      const output =
        join(
          outputs,
          'animation é.gif',
        );

      run(
        engines.ffmpeg,
        [
          '-hide_banner',
          '-loglevel',
          'error',
          '-y',
          '-i',
          prepared.video,
          '-vf',
          (
            'fps=8,' +
            'scale=120:-1:' +
            'flags=lanczos'
          ),
          output,
        ],
      );

      const magic =
        readFileSync(output)
          .subarray(0, 6)
          .toString('ascii');

      if (
        magic !== 'GIF87a' &&
        magic !== 'GIF89a'
      ) {
        throw new Error(
          `invalid GIF header: ${magic}`,
        );
      }
    },
  );

  // ==========================================================
  // ARCHIVES — vérification contenu + SHA après extraction
  // ==========================================================

  await gateCase(
    'archive-zip-roundtrip',
    () => {

      const archive =
        join(
          outputs,
          'archive é.zip',
        );

      const extracted =
        join(
          outputs,
          'zip extrait é',
        );

      run(
        engines.archive,
        [
          'a',
          '-tzip',
          archive,
          basename(
            prepared.archiveDir,
          ),
        ],
        {
          cwd:
            dirname(
              prepared.archiveDir,
            ),
        },
      );

      mkdirSync(
        extracted,
        {
          recursive: true,
        },
      );

      run(
        engines.archive,
        [
          'x',
          archive,
          `-o${extracted}`,
          '-y',
        ],
      );

      assertTreesEqual(
        prepared.archiveDir,
        join(
          extracted,
          basename(
            prepared.archiveDir,
          ),
        ),
      );
    },
  );

  await gateCase(
    'tar-zst-roundtrip',
    () => {
      archiveCompressedRoundtrip(
        'zst',
      );
    },
  );

  await gateCase(
    'tar-lz4-roundtrip',
    () => {
      archiveCompressedRoundtrip(
        'lz4',
      );
    },
  );

  // ==========================================================
  // HTML -> PDF via vrai navigateur headless
  // ==========================================================

  await gateCase(
    'html-to-pdf',
    () => {

      if (!engines.browser) {

        if (
          (
            caseDefinition(
              'html-to-pdf',
            ).optionalOn ||
            []
          ).includes(target)
        ) {
          return {
            skipped: true,
            reason:
              (
                'browser unavailable on ' +
                `optional target ${target}`
              ),
          };
        }

        throw new Error(
          'headless browser missing for ' +
          'required HTML -> PDF conversion',
        );
      }

      const output =
        join(
          outputs,
          'page html é.pdf',
        );

      browserPrint(
        prepared.html,
        output,
      );

      assertPdf(
        output,
        1,
      );

      assertPdfText(
        output,
        'FILEFLOW_HTML_GATE',
      );
    },
  );

  // ==========================================================
  // ERREUR ATTENDUE
  // ==========================================================

  await gateCase(
    'invalid-image-rejected',
    () => {

      const output =
        join(
          outputs,
          'ne doit pas exister é.webp',
        );

      const result =
        execute(
          engines.vips,
          [
            'copy',
            prepared.brokenImage,
            output,
          ],
          {
            capture: true,
          },
        );

      if (
        !result.error &&
        result.status === 0
      ) {
        throw new Error(
          'corrupted image conversion unexpectedly succeeded',
        );
      }

      if (
        existsSync(output) &&
        statSync(output).size > 0
      ) {
        throw new Error(
          'failed conversion left a non-empty output',
        );
      }
    },
  );

  // ==========================================================
  // ATTESTATION
  // ==========================================================

  const failed =
    results.filter(
      (item) =>
        item.status === 'failed',
    );

  const skipped =
    results.filter(
      (item) =>
        item.status === 'skipped',
    );

  if (failed.length) {
    throw new Error(
      `${failed.length} conversion ` +
      `gate case(s) failed`,
    );
  }

  const attestation = {
    schemaVersion: 1,

    status: 'passed',

    sourceSha,

    version,

    target,

    runner: {
      platform:
        process.platform,

      arch:
        process.arch,
    },

    /*
     * SHA de l'artefact exact qui sera ensuite recherché
     * byte-for-byte dans le set de publication.
     */
    candidateArtifact:
      artifactRecord(
        candidateArtifact,
      ),

    setupCli:
      artifactRecord(
        setupCli,
      ),

    executorContractActions:
      [
        ...new Set(
          corpus.cases
            .map(
              (item) =>
                item.actionId,
            )
            .filter(Boolean),
        ),
      ].sort(),

    executorTests:
      'required-by-native-release-workflow',

    engines:
      Object.fromEntries(
        Object.entries(
          engines,
        ).map(
          ([id, path]) => [
            id,
            path
              ? basename(path)
              : null,
          ],
        ),
      ),

    total:
      results.length,

    passed:
      results.filter(
        (item) =>
          item.status ===
          'passed',
      ).length,

    skipped:
      skipped.length,

    failed: 0,

    cases:
      results,

    completedAt:
      new Date().toISOString(),
  };

  const output =
    resolve(
      outputArg ||
      join(
        repo,
        'dist/conversion-attestations',
        `${target}.json`,
      ),
    );

  mkdirSync(
    dirname(output),
    {
      recursive: true,
    },
  );

  writeFileSync(
    output,
    (
      JSON.stringify(
        attestation,
        null,
        2,
      ) + '\n'
    ),
  );

  console.log(
    `[conversion-e2e] PASS ` +
    `${attestation.passed}/` +
    `${attestation.total}, ` +
    `skipped=${attestation.skipped} ` +
    `-> ${output}`,
  );

} finally {

  rmSync(
    workRoot,
    {
      recursive: true,
      force: true,
    },
  );
}

/* ==========================================================
 * HELPERS
 * ========================================================== */

function parseArgs(values) {

  const map =
    new Map();

  for (
    let i = 0;
    i < values.length;
    i += 2
  ) {

    const key =
      values[i];

    const value =
      values[i + 1];

    if (
      !key?.startsWith('--') ||
      value == null
    ) {
      throw new Error(
        `invalid argument list near ` +
        `${key ?? '<end>'}`,
      );
    }

    map.set(
      key,
      value,
    );
  }

  return map;
}

function requiredArg(name) {

  const value =
    argv.get(name);

  if (!value) {
    throw new Error(
      `missing required argument ${name}`,
    );
  }

  return value;
}

function execute(
  program,
  args = [],
  options = {},
) {

  return spawnSync(
    program,
    args,
    {
      cwd:
        options.cwd ||
        repo,

      env: {
        ...process.env,
        ...(options.env || {}),
      },

      encoding:
        'utf8',

      stdio:
        options.capture
          ? [
              'ignore',
              'pipe',
              'pipe',
            ]
          : 'inherit',

      windowsHide:
        true,

      timeout:
        options.timeoutMs ||
        120_000,
    },
  );
}

function run(
  program,
  args = [],
  options = {},
) {

  console.log(
    `+ ${basename(program)} ` +
    args
      .map(display)
      .join(' '),
  );

  const result =
    execute(
      program,
      args,
      options,
    );

  if (
    result.error ||
    result.status !== 0
  ) {

    throw new Error(
      `${program} failed (` +
      `${result.status ?? 'spawn'}): ` +
      (
        result.error?.message ||
        result.stderr ||
        result.stdout ||
        ''
      ),
    );
  }

  return result;
}

function capture(
  program,
  args = [],
  options = {},
) {

  const result =
    run(
      program,
      args,
      {
        ...options,
        capture: true,
      },
    );

  return (
    result.stdout ||
    ''
  );
}

function display(value) {

  return (
    /\s|[éèàù]/u.test(value)
      ? JSON.stringify(value)
      : value
  );
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

/* ==========================================================
 * RELEASE ARTIFACT
 * ========================================================== */

function selectCandidateArtifact(
  directory,
  extension,
) {

  const files =
    walk(directory)
      .filter(
        (path) => {

          const name =
            basename(path)
              .toLowerCase();

          if (
            name.includes(
              'fileflowsetup',
            ) ||
            name.includes(
              'fileflow-setup',
            )
          ) {
            return false;
          }

          if (
            extension === '.exe'
          ) {

            return (
              name.endsWith(
                '-setup.exe',
              ) &&
              !name.endsWith(
                '.sig',
              )
            );
          }

          return (
            name.endsWith(
              extension,
            ) &&
            !name.endsWith(
              `${extension}.sig`,
            )
          );
        },
      );

  if (
    files.length !== 1
  ) {
    throw new Error(
      `expected one ${extension} ` +
      `candidate in ${directory}; ` +
      `found ${files.join(', ') || 'none'}`,
    );
  }

  return files[0];
}

function selectSetupCli(directory) {

  const files =
    walk(directory)
      .filter(
        (path) => (
          /FileFlowSetupCLI_/i.test(
            basename(path),
          ) &&
          !/\.(sig|sha256)$/i.test(
            path,
          )
        ),
      );

  if (
    files.length !== 1
  ) {
    throw new Error(
      `expected one packaged Setup CLI; ` +
      `found ${files.join(', ') || 'none'}`,
    );
  }

  return files[0];
}

function makeExecutable(path) {

  if (
    process.platform !== 'win32'
  ) {

    chmodSync(
      path,
      statSync(path).mode |
      0o111,
    );
  }
}

/* ==========================================================
 * WINDOWS PATH REFRESH
 * ========================================================== */

function refreshWindowsPath() {

  const script =
    (
      "& { " +
      "$m=[Environment]::GetEnvironmentVariable(" +
      "'Path','Machine'); " +
      "$u=[Environment]::GetEnvironmentVariable(" +
      "'Path','User'); " +
      "Write-Output ($m+';'+$u) " +
      "}"
    );

  const refreshed =
    capture(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        script,
      ],
    ).trim();

  if (refreshed) {
    process.env.PATH =
      `${refreshed};` +
      `${process.env.PATH || ''}`;
  }
}

/* ==========================================================
 * ENGINE DISCOVERY
 * ========================================================== */

function whereProgram(
  names,
  explicit = [],
) {

  for (
    const path of explicit
  ) {

    if (
      path &&
      existsSync(path)
    ) {
      return path;
    }
  }

  for (
    const name of names
  ) {

    const locator =
      process.platform === 'win32'
        ? execute(
            'where.exe',
            [name],
            {
              capture: true,
            },
          )
        : execute(
            'which',
            [name],
            {
              capture: true,
            },
          );

    if (
      !locator.error &&
      locator.status === 0
    ) {

      const path =
        (
          locator.stdout ||
          ''
        )
          .split(/\r?\n/)
          .map(
            (value) =>
              value.trim(),
          )
          .find(Boolean);

      if (
        path &&
        existsSync(path)
      ) {
        return path;
      }
    }
  }

  return null;
}

function resolveEngines() {

  const programFiles =
    process.env.ProgramFiles ||
    'C:\\Program Files';

  const programFilesX86 =
    process.env['ProgramFiles(x86)'] ||
    'C:\\Program Files (x86)';

  const local =
    process.env.LOCALAPPDATA ||
    join(
      homedir(),
      'AppData',
      'Local',
    );

  const office =
    whereProgram(
      [
        'soffice',
        'libreoffice',
      ],
      [
        (
          process.platform === 'darwin'
            ? (
                '/Applications/' +
                'LibreOffice.app/' +
                'Contents/MacOS/soffice'
              )
            : null
        ),
        (
          process.platform === 'win32'
            ? join(
                programFiles,
                'LibreOffice',
                'program',
                'soffice.exe',
              )
            : null
        ),
        (
          process.platform === 'win32'
            ? join(
                programFilesX86,
                'LibreOffice',
                'program',
                'soffice.exe',
              )
            : null
        ),
        (
          process.platform === 'win32'
            ? join(
                local,
                'Programs',
                'LibreOffice',
                'program',
                'soffice.exe',
              )
            : null
        ),
      ],
    );

  return {
    vips:
      whereProgram(
        ['vips'],
      ),

    vipsheader:
      whereProgram(
        ['vipsheader'],
      ),

    img2pdf:
      whereProgram(
        process.platform === 'win32'
          ? [
              'img2pdf.exe',
              'img2pdf',
            ]
          : [
              'img2pdf',
            ],
      ),

    qpdf:
      whereProgram(
        ['qpdf'],
      ),

    pdfinfo:
      whereProgram(
        ['pdfinfo'],
      ),

    pdftoppm:
      whereProgram(
        ['pdftoppm'],
      ),

    pdftotext:
      whereProgram(
        ['pdftotext'],
      ),

    office,

    exiftool:
      whereProgram(
        process.platform === 'win32'
          ? [
              'exiftool.exe',
              'exiftool',
            ]
          : [
              'exiftool',
            ],
      ),

    ffmpeg:
      whereProgram(
        ['ffmpeg'],
      ),

    ffprobe:
      whereProgram(
        ['ffprobe'],
      ),

    archive:
      whereProgram(
        process.platform === 'win32'
          ? [
              '7zz.exe',
              '7z.exe',
              '7za.exe',
              '7zz',
              '7z',
            ]
          : [
              '7zz',
              '7z',
              '7za',
            ],
      ),

    pandoc:
      whereProgram(
        ['pandoc'],
      ),

    zstd:
      whereProgram(
        ['zstd'],
      ),

    lz4:
      whereProgram(
        ['lz4'],
      ),

    browser:
      resolveBrowser(),
  };
}

function resolveBrowser() {

  if (
    process.platform === 'darwin'
  ) {

    return whereProgram(
      [
        'google-chrome',
        'chromium',
      ],
      [
        (
          '/Applications/' +
          'Google Chrome.app/' +
          'Contents/MacOS/' +
          'Google Chrome'
        ),
        (
          '/Applications/' +
          'Chromium.app/' +
          'Contents/MacOS/' +
          'Chromium'
        ),
        (
          '/Applications/' +
          'Microsoft Edge.app/' +
          'Contents/MacOS/' +
          'Microsoft Edge'
        ),
      ],
    );
  }

  if (
    process.platform === 'win32'
  ) {

    const pf =
      process.env.ProgramFiles ||
      'C:\\Program Files';

    const pfx =
      process.env['ProgramFiles(x86)'] ||
      'C:\\Program Files (x86)';

    const local =
      process.env.LOCALAPPDATA ||
      join(
        homedir(),
        'AppData',
        'Local',
      );

    return whereProgram(
      [
        'msedge.exe',
        'chrome.exe',
        'chromium.exe',
      ],
      [
        join(
          pf,
          'Microsoft',
          'Edge',
          'Application',
          'msedge.exe',
        ),
        join(
          pfx,
          'Microsoft',
          'Edge',
          'Application',
          'msedge.exe',
        ),
        join(
          pf,
          'Google',
          'Chrome',
          'Application',
          'chrome.exe',
        ),
        join(
          pfx,
          'Google',
          'Chrome',
          'Application',
          'chrome.exe',
        ),
        join(
          local,
          'Google',
          'Chrome',
          'Application',
          'chrome.exe',
        ),
      ],
    );
  }

  return whereProgram(
    [
      'google-chrome',
      'google-chrome-stable',
      'chromium',
      'chromium-browser',
    ],
  );
}

/* ==========================================================
 * FIXTURE PREPARATION
 * ========================================================== */

function prepareFixtures() {

  const ppm =
    join(
      fixtureCopy,
      'image couleur é.ppm',
    );

  const png1 =
    join(
      workRoot,
      'image couleur é.png',
    );

  const png2 =
    join(
      workRoot,
      'deuxième image é.png',
    );

  const jpeg =
    join(
      workRoot,
      'image metadata é.jpg',
    );

  run(
    engines.vips,
    [
      'copy',
      ppm,
      png1,
    ],
  );

  run(
    engines.vips,
    [
      'copy',
      ppm,
      png2,
    ],
  );

  run(
    engines.vips,
    [
      'copy',
      ppm,
      jpeg,
    ],
  );

  assertPng(png1);

  assertImageDimensions(
    png1,
    96,
    64,
  );

  const wav =
    join(
      workRoot,
      'son source é.wav',
    );

  run(
    engines.ffmpeg,
    [
      '-hide_banner',
      '-loglevel',
      'error',
      '-y',
      '-f',
      'lavfi',
      '-i',
      (
        'sine=frequency=880:' +
        'sample_rate=44100'
      ),
      '-t',
      '1',
      wav,
    ],
  );

  const video =
    join(
      workRoot,
      'vidéo source é.mkv',
    );

  run(
    engines.ffmpeg,
    [
      '-hide_banner',
      '-loglevel',
      'error',
      '-y',
      '-f',
      'lavfi',
      '-i',
      (
        'testsrc=' +
        'size=160x120:' +
        'rate=12'
      ),
      '-f',
      'lavfi',
      '-i',
      (
        'sine=frequency=660:' +
        'sample_rate=44100'
      ),
      '-t',
      '1',
      '-c:v',
      'mpeg4',
      '-q:v',
      '5',
      '-c:a',
      'aac',
      '-shortest',
      video,
    ],
  );

  const info =
    ffprobe(video);

  requireStream(
    info,
    'video',
  );

  requireStream(
    info,
    'audio',
  );

  prepared = {
    png1,
    png2,
    jpeg,
    wav,
    video,

    rtf:
      join(
        fixtureCopy,
        'document bureautique é.rtf',
      ),

    markdown:
      join(
        fixtureCopy,
        'document test é.md',
      ),

    html:
      join(
        fixtureCopy,
        'page dynamique é.html',
      ),

    brokenImage:
      join(
        fixtureCopy,
        'broken-image.jpg',
      ),

    archiveDir:
      join(
        fixtureCopy,
        'archive source é',
      ),
  };
}

/* ==========================================================
 * CASE RUNNER + PERFORMANCE BUDGET
 * ========================================================== */

async function gateCase(
  id,
  fn,
) {

  const definition =
    caseDefinition(id);

  const started =
    Date.now();

  try {

    const outcome =
      await fn();

    const durationMs =
      Date.now() -
      started;

    if (
      outcome?.skipped
    ) {

      results.push({
        id,

        actionId:
          definition.actionId ||
          null,

        status:
          'skipped',

        durationMs,

        budgetMs:
          definition.budgetMs,

        reason:
          outcome.reason,
      });

      console.log(
        `[conversion-e2e] SKIP ` +
        `${id}: ${outcome.reason}`,
      );

      return;
    }

    if (
      durationMs >
      definition.budgetMs
    ) {

      throw new Error(
        `performance budget exceeded: ` +
        `${durationMs}ms > ` +
        `${definition.budgetMs}ms`,
      );
    }

    results.push({
      id,

      actionId:
        definition.actionId ||
        null,

      status:
        'passed',

      durationMs,

      budgetMs:
        definition.budgetMs,
    });

    console.log(
      `[conversion-e2e] PASS ` +
      `${id} ${durationMs}ms`,
    );

  } catch (error) {

    const durationMs =
      Date.now() -
      started;

    results.push({
      id,

      actionId:
        definition.actionId ||
        null,

      status:
        'failed',

      durationMs,

      budgetMs:
        definition.budgetMs,

      error:
        String(
          error?.message ||
          error,
        ),
    });

    console.error(
      `[conversion-e2e] FAIL ` +
      `${id}: ` +
      `${error?.stack || error}`,
    );

    throw error;
  }
}

function caseDefinition(id) {

  const item =
    corpus.cases.find(
      (entry) =>
        entry.id === id,
    );

  if (!item) {
    throw new Error(
      `case missing from corpus: ${id}`,
    );
  }

  return item;
}

/* ==========================================================
 * SEMANTIC OUTPUT VALIDATION
 * ========================================================== */

function assertFile(
  path,
  minBytes = 1,
) {

  if (!existsSync(path)) {
    throw new Error(
      `output missing: ${path}`,
    );
  }

  const size =
    statSync(path).size;

  if (
    size < minBytes
  ) {
    throw new Error(
      `output too small ` +
      `(${size} bytes): ${path}`,
    );
  }
}

function assertPng(path) {

  assertFile(
    path,
    16,
  );

  const header =
    readFileSync(path)
      .subarray(0, 8)
      .toString('hex');

  if (
    header !==
    '89504e470d0a1a0a'
  ) {
    throw new Error(
      `invalid PNG header: ${header}`,
    );
  }
}

function assertWebp(path) {

  assertFile(
    path,
    16,
  );

  const bytes =
    readFileSync(path)
      .subarray(0, 12);

  if (
    bytes
      .subarray(0, 4)
      .toString('ascii') !==
      'RIFF' ||
    bytes
      .subarray(8, 12)
      .toString('ascii') !==
      'WEBP'
  ) {
    throw new Error(
      'invalid WebP RIFF header',
    );
  }
}

function imageDimensions(path) {

  const width =
    Number(
      capture(
        engines.vipsheader,
        [
          '-f',
          'width',
          path,
        ],
      ).trim(),
    );

  const height =
    Number(
      capture(
        engines.vipsheader,
        [
          '-f',
          'height',
          path,
        ],
      ).trim(),
    );

  if (
    !Number.isInteger(width) ||
    !Number.isInteger(height) ||
    width < 1 ||
    height < 1
  ) {
    throw new Error(
      `invalid dimensions for ${path}`,
    );
  }

  return {
    width,
    height,
  };
}

function assertImageDimensions(
  path,
  expectedWidth,
  expectedHeight,
) {

  const {
    width,
    height,
  } =
    imageDimensions(path);

  if (
    width !== expectedWidth ||
    height !== expectedHeight
  ) {
    throw new Error(
      `dimensions mismatch: ` +
      `${width}x${height} != ` +
      `${expectedWidth}x${expectedHeight}`,
    );
  }
}

function assertPdf(
  path,
  expectedPages = null,
) {

  assertFile(
    path,
    32,
  );

  if (
    readFileSync(path)
      .subarray(0, 5)
      .toString('ascii') !==
    '%PDF-'
  ) {
    throw new Error(
      `PDF magic bytes missing: ${path}`,
    );
  }

  run(
    engines.qpdf,
    [
      '--check',
      path,
    ],
    {
      capture: true,
    },
  );

  const info =
    capture(
      engines.pdfinfo,
      [
        path,
      ],
    );

  const pages =
    Number(
      info.match(
        /^Pages:\s+(\d+)/m,
      )?.[1] ||
      0,
    );

  if (
    pages < 1
  ) {
    throw new Error(
      `pdfinfo returned invalid ` +
      `page count for ${path}`,
    );
  }

  if (
    expectedPages != null &&
    pages !== expectedPages
  ) {
    throw new Error(
      `PDF page count ${pages} != ` +
      `${expectedPages}`,
    );
  }
}

function assertPdfText(
  path,
  marker,
) {

  const text =
    capture(
      engines.pdftotext,
      [
        path,
        '-',
      ],
    );

  if (
    !text.includes(marker)
  ) {
    throw new Error(
      `PDF semantic marker missing: ${marker}`,
    );
  }
}

function ffprobe(path) {

  assertFile(
    path,
    16,
  );

  const value =
    capture(
      engines.ffprobe,
      [
        '-v',
        'error',
        '-show_streams',
        '-show_format',
        '-of',
        'json',
        path,
      ],
    );

  return JSON.parse(value);
}

function requireStream(
  info,
  type,
) {

  if (
    !info.streams?.some(
      (stream) =>
        stream.codec_type === type,
    )
  ) {
    throw new Error(
      `ffprobe missing ${type} stream`,
    );
  }
}

function requireDuration(
  info,
  min,
  max,
) {

  const duration =
    Number(
      info.format?.duration ||
      info.streams?.find(
        (stream) =>
          stream.duration,
      )?.duration ||
      0,
    );

  if (
    !(
      duration >= min &&
      duration <= max
    )
  ) {
    throw new Error(
      `duration ${duration} outside ` +
      `${min}-${max}s`,
    );
  }
}

/* ==========================================================
 * BROWSER
 * ========================================================== */

function browserPrint(
  input,
  output,
) {

  const url =
    pathToFileURL(input).href;

  const common = [
    '--disable-gpu',
    '--disable-dev-shm-usage',
    '--no-sandbox',
    '--disable-extensions',
    '--disable-sync',
    `--print-to-pdf=${output}`,
    url,
  ];

  let result =
    execute(
      engines.browser,
      [
        '--headless=new',
        ...common,
      ],
      {
        capture: true,
        timeoutMs: 45_000,
      },
    );

  if (
    result.error ||
    result.status !== 0 ||
    !existsSync(output)
  ) {

    rmSync(
      output,
      {
        force: true,
      },
    );

    result =
      execute(
        engines.browser,
        [
          '--headless',
          ...common,
        ],
        {
          capture: true,
          timeoutMs: 45_000,
        },
      );
  }

  if (
    result.error ||
    result.status !== 0
  ) {
    throw new Error(
      `browser print failed: ` +
      (
        result.error?.message ||
        result.stderr ||
        result.stdout ||
        result.status
      ),
    );
  }
}

/* ==========================================================
 * TAR.ZST / TAR.LZ4
 * ========================================================== */

function archiveCompressedRoundtrip(
  kind,
) {

  const tar =
    join(
      outputs,
      `archive ${kind} é.tar`,
    );

  run(
    engines.archive,
    [
      'a',
      '-ttar',
      tar,
      basename(
        prepared.archiveDir,
      ),
    ],
    {
      cwd:
        dirname(
          prepared.archiveDir,
        ),
    },
  );

  const compressed =
    `${tar}.${kind}`;

  const restoredTar =
    join(
      outputs,
      (
        `archive ${kind} ` +
        `restaurée é.tar`
      ),
    );

  if (
    kind === 'zst'
  ) {

    run(
      engines.zstd,
      [
        '-f',
        tar,
        '-o',
        compressed,
      ],
    );

    run(
      engines.zstd,
      [
        '-d',
        '-f',
        compressed,
        '-o',
        restoredTar,
      ],
    );

  } else {

    run(
      engines.lz4,
      [
        '-f',
        tar,
        compressed,
      ],
    );

    run(
      engines.lz4,
      [
        '-d',
        '-f',
        compressed,
        restoredTar,
      ],
    );
  }

  const extracted =
    join(
      outputs,
      (
        `archive ${kind} ` +
        `extraite é`
      ),
    );

  mkdirSync(
    extracted,
    {
      recursive: true,
    },
  );

  run(
    engines.archive,
    [
      'x',
      restoredTar,
      `-o${extracted}`,
      '-y',
    ],
  );

  assertTreesEqual(
    prepared.archiveDir,
    join(
      extracted,
      basename(
        prepared.archiveDir,
      ),
    ),
  );
}

/* ==========================================================
 * ARCHIVE CONTENT HASHES
 * ========================================================== */

function treeHashes(root) {

  const files =
    walk(root)
      .sort();

  return Object.fromEntries(
    files.map(
      (path) => [
        path
          .slice(
            root.length + 1,
          )
          .replaceAll(
            '\\',
            '/',
          ),

        sha256(path),
      ],
    ),
  );
}

function assertTreesEqual(
  expected,
  actual,
) {

  if (
    !existsSync(actual)
  ) {
    throw new Error(
      `extracted directory missing: ${actual}`,
    );
  }

  const left =
    treeHashes(expected);

  const right =
    treeHashes(actual);

  if (
    JSON.stringify(left) !==
    JSON.stringify(right)
  ) {
    throw new Error(
      `archive roundtrip mismatch\n` +
      `expected=${JSON.stringify(left)}\n` +
      `actual=${JSON.stringify(right)}`,
    );
  }
}

/* ==========================================================
 * SHA / ARTIFACT
 * ========================================================== */

function sha256(path) {

  return createHash('sha256')
    .update(
      readFileSync(path),
    )
    .digest('hex');
}

function artifactRecord(path) {

  return {
    name:
      basename(path),

    sha256:
      sha256(path),

    size:
      statSync(path).size,
  };
}
