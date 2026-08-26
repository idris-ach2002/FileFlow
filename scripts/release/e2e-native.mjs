#!/usr/bin/env node

import { createHash } from 'node:crypto';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import {
  basename,
  dirname,
  join,
  resolve,
} from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const repo = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../..',
);

const args = new Map();

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

const target = args.get('--target');

const version = (
  args.get('--version') || ''
).replace(/^v/, '');

if (
  !target ||
  !/^\d+\.\d+\.\d+$/.test(version)
) {
  throw new Error(
    'usage: e2e-native.mjs --target <triple> --version X.Y.Z',
  );
}

const targets = new Map([
  [
    'aarch64-apple-darwin',
    {
      os: 'darwin',
      arch: 'arm64',
      key: 'darwin-aarch64',
      primary: '.dmg',
    },
  ],
  [
    'x86_64-apple-darwin',
    {
      os: 'darwin',
      arch: 'x64',
      key: 'darwin-x86_64',
      primary: '.dmg',
    },
  ],
  [
    'x86_64-pc-windows-msvc',
    {
      os: 'win32',
      arch: 'x64',
      key: 'windows-x86_64',
      primary: '.exe',
    },
  ],
  [
    'x86_64-unknown-linux-gnu',
    {
      os: 'linux',
      arch: 'x64',
      key: 'linux-x86_64',
      primary: '.appimage',
    },
  ],
  [
    'aarch64-unknown-linux-gnu',
    {
      os: 'linux',
      arch: 'arm64',
      key: 'linux-aarch64',
      primary: '.appimage',
    },
  ],
]);

const descriptor = targets.get(target);

if (!descriptor) {
  throw new Error(
    `unsupported E2E target: ${target}`,
  );
}

if (
  process.platform !== descriptor.os ||
  process.arch !== descriptor.arch
) {
  throw new Error(
    `native runner mismatch: ` +
    `${process.platform}/${process.arch} ` +
    `for ${target}`,
  );
}

const sourceSha = capture(
  'git',
  ['rev-parse', 'HEAD'],
).trim();

if (!/^[0-9a-f]{40}$/.test(sourceSha)) {
  throw new Error(
    `invalid source SHA: ${sourceSha}`,
  );
}

const mainBundle = resolve(
  repo,
  'target',
  target,
  'release',
  'bundle',
);

const setupBundle = resolve(
  repo,
  'target',
  'fileflow-setup',
  target,
  'release',
  'bundle',
);

const currentApplication =
  selectPrimaryApplication(
    mainBundle,
    descriptor.primary,
  );

const releaseSetupCli =
  selectSetupCli(setupBundle);

makeExecutable(releaseSetupCli);

const testedArtifacts = [
  artifactRecord(
    'application-primary',
    currentApplication,
  ),
  artifactRecord(
    'setup-cli',
    releaseSetupCli,
  ),
];

const currentApplicationSha =
  sha256(currentApplication);

const releaseSetupCliSha =
  sha256(releaseSetupCli);

console.log(
  `[e2e] target=${target}`,
);

console.log(
  `[e2e] source=${sourceSha}`,
);

console.log(
  `[e2e] version=${version}`,
);

console.log(
  `[e2e] application=${currentApplication}`,
);

console.log(
  `[e2e] setup-cli=${releaseSetupCli}`,
);

/*
 * IMPORTANT SECURITY MODEL
 *
 * The candidate application has not been published yet, so the
 * production downloads.json cannot legitimately point to it.
 *
 * We therefore build a DEBUG Setup CLI only for the pre-publication
 * install/upgrade harness. FILEFLOW_SETUP_LOCAL_APPLICATION remains
 * guarded by #[cfg(debug_assertions)] in adapter.rs.
 *
 * The exact RELEASE Setup CLI produced for publication is then used
 * for repair and uninstall.
 *
 * This means:
 *
 *   candidate artifact bytes = exact bytes tested
 *   Setup install logic       = real Setup core
 *   repair/uninstall binary   = exact release CLI
 *   production local bypass   = impossible
 */

const debugCli =
  buildDebugSetupCli();

const previous =
  await downloadPreviousStable();

console.log(
  `[e2e] previous stable=${previous.version}`,
);

console.log(
  `[e2e] previous application=${previous.path}`,
);

cleanMachine();

const scenarios = [];

/* ==========================================================
 * A. CURRENT CLEAN INSTALL
 * ========================================================== */

console.log(
  '\n[e2e] 1/7 clean install candidate',
);

runLocalSetup(
  debugCli,
  currentApplication,
  version,
  [
    'install',
    '--app-only',
    '--yes',
    '--no-launch',
  ],
);

verifyInstalled(version);

/*
 * SystemSetupAdapter performs its installed-application
 * smoke launch during postcheck even with --no-launch.
 *
 * --no-launch only suppresses the final user-facing launch.
 */
scenarios.push(
  'clean-install-exact-artifact',
);

scenarios.push(
  'installed-postcheck-launch',
);

/* ==========================================================
 * B. DELIBERATE DAMAGE + REPAIR
 * ========================================================== */

console.log(
  '\n[e2e] 2/7 deliberate damage + repair',
);

const damage =
  damageIntegration();

if (descriptor.os === 'darwin') {

  /*
   * macOS repair needs the not-yet-public candidate DMG,
   * therefore the debug-only local source harness is used.
   */

  runLocalSetup(
    debugCli,
    currentApplication,
    version,
    [
      'repair',
      '--yes',
      '--no-launch',
    ],
  );

  if (
    damage?.backup &&
    existsSync(damage.backup)
  ) {
    rmSync(
      damage.backup,
      {
        recursive: true,
        force: true,
      },
    );
  }

} else {

  /*
   * Windows/Linux integration-only repair does not need to
   * download an application. Use the EXACT packaged release
   * Setup CLI.
   */

  run(
    releaseSetupCli,
    [
      'repair',
      '--yes',
      '--no-launch',
    ],
  );
}

verifyInstalled(version);

scenarios.push(
  'repair-after-deliberate-damage',
);

/* ==========================================================
 * C. UNINSTALL VIA EXACT RELEASE SETUP CLI
 * ========================================================== */

console.log(
  '\n[e2e] 3/7 packaged CLI uninstall',
);

run(
  releaseSetupCli,
  [
    'uninstall',
    '--keep-engines',
    '--yes',
  ],
);

verifyClean();

scenarios.push(
  'packaged-cli-uninstall',
);

/*
 * Remove receipt/maintenance state from the ephemeral runner
 * before intentionally installing N-1.
 */
cleanMachine();

/* ==========================================================
 * D. INSTALL N-1
 * ========================================================== */

console.log(
  `\n[e2e] 4/7 install previous stable ${previous.version}`,
);

runLocalSetup(
  debugCli,
  previous.path,
  previous.version,
  [
    'install',
    '--app-only',
    '--yes',
    '--no-launch',
  ],
);

verifyInstalled(
  previous.version,
);

scenarios.push(
  'install-public-n-minus-1',
);

/* ==========================================================
 * E. REAL N-1 -> N UPGRADE
 * ========================================================== */

console.log(
  `\n[e2e] 5/7 upgrade ` +
  `${previous.version} -> ${version}`,
);

runLocalSetup(
  debugCli,
  currentApplication,
  version,
  [
    'install',
    '--app-only',
    '--yes',
    '--no-launch',
  ],
);

verifyInstalled(version);

scenarios.push(
  'upgrade-n-minus-1-to-candidate',
);

/* ==========================================================
 * F. UNINSTALL AFTER UPGRADE
 * ========================================================== */

console.log(
  '\n[e2e] 6/7 uninstall after upgrade',
);

run(
  releaseSetupCli,
  [
    'uninstall',
    '--keep-engines',
    '--yes',
  ],
);

verifyClean();

scenarios.push(
  'post-upgrade-uninstall',
);

cleanMachine();

/* ==========================================================
 * G. SECONDARY NATIVE PACKAGE FORMATS
 * ========================================================== */

console.log(
  '\n[e2e] 7/7 native alternate package formats',
);

if (descriptor.os === 'win32') {

  testWindowsMsiPackages();

  scenarios.push(
    'windows-msi-install-uninstall',
  );

} else if (
  descriptor.os === 'linux'
) {

  testLinuxDebPackages();

  testLinuxRpmPackages();

  scenarios.push(
    'linux-deb-install-uninstall',
  );

  scenarios.push(
    'linux-rpm-fedora-install-uninstall',
  );

} else {

  /*
   * DMG is already the primary candidate artifact and was
   * mounted/installed through Setup during the scenarios above.
   */
  scenarios.push(
    'macos-dmg-native-install-covered-by-setup',
  );
}

/* ==========================================================
 * BYTE-FOR-BYTE IMMUTABILITY
 * ========================================================== */

if (
  sha256(currentApplication) !==
  currentApplicationSha
) {
  throw new Error(
    'candidate application artifact changed during E2E',
  );
}

if (
  sha256(releaseSetupCli) !==
  releaseSetupCliSha
) {
  throw new Error(
    'packaged Setup CLI changed during E2E',
  );
}

/* ==========================================================
 * E2E ATTESTATION
 * ========================================================== */

const attestationDirectory =
  resolve(
    repo,
    'dist',
    'e2e-attestations',
  );

mkdirSync(
  attestationDirectory,
  {
    recursive: true,
  },
);

const attestation = {
  schemaVersion: 1,

  status: 'passed',

  sourceSha,

  version,

  target,

  runner: {
    platform: process.platform,
    arch: process.arch,
  },

  previousVersion:
    previous.version,

  testedArtifacts,

  scenarios,

  completedAt:
    new Date().toISOString(),
};

const attestationPath =
  join(
    attestationDirectory,
    `${target}.json`,
  );

writeFileSync(
  attestationPath,
  `${JSON.stringify(
    attestation,
    null,
    2,
  )}\n`,
);

console.log(
  `[e2e] PASS ${target} -> ` +
  `${attestationPath}`,
);

/* ==========================================================
 * HELPERS
 * ========================================================== */

function run(
  program,
  commandArgs = [],
  options = {},
) {

  console.log(
    `+ ${program} ` +
    commandArgs
      .map(shellDisplay)
      .join(' '),
  );

  const result =
    spawnSync(
      program,
      commandArgs,
      {
        cwd:
          options.cwd ||
          repo,

        env: {
          ...process.env,
          ...(options.env || {}),
        },

        encoding: 'utf8',

        stdio:
          options.capture
            ? [
                'ignore',
                'pipe',
                'pipe',
              ]
            : 'inherit',

        windowsHide: true,
      },
    );

  if (result.error) {
    throw result.error;
  }

  const accepted =
    options.accepted ||
    [0];

  if (
    !accepted.includes(
      result.status ?? -1,
    )
  ) {

    const details =
      options.capture
        ? (
            `\nstdout:\n` +
            `${result.stdout || ''}` +
            `\nstderr:\n` +
            `${result.stderr || ''}`
          )
        : '';

    throw new Error(
      `${program} exited with ` +
      `${result.status}${details}`,
    );
  }

  return result;
}

function capture(
  program,
  commandArgs = [],
  options = {},
) {

  const result =
    run(
      program,
      commandArgs,
      {
        ...options,
        capture: true,
      },
    );

  return result.stdout || '';
}

function shellDisplay(value) {

  return (
    /[\s'"éèà]/u.test(value)
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

function selectPrimaryApplication(
  directory,
  extension,
) {

  const files =
    walk(directory)
      .filter((path) => {

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

        if (extension === '.exe') {

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
      });

  if (files.length !== 1) {

    throw new Error(
      `expected one primary ` +
      `${extension} in ${directory}, ` +
      `found: ` +
      `${files.join(', ') || 'none'}`,
    );
  }

  return files[0];
}

function selectSetupCli(directory) {

  const files =
    walk(directory)
      .filter((path) => (
        /FileFlowSetupCLI_/i.test(
          basename(path),
        ) &&
        !/\.(sig|sha256)$/i.test(
          path,
        )
      ));

  if (files.length !== 1) {

    throw new Error(
      `expected one packaged ` +
      `Setup CLI, found: ` +
      `${files.join(', ') || 'none'}`,
    );
  }

  return files[0];
}

function selectOne(
  directory,
  predicate,
  label,
) {

  const files =
    walk(directory)
      .filter(predicate);

  if (files.length !== 1) {

    throw new Error(
      `expected one ${label}, ` +
      `found: ` +
      `${files.join(', ') || 'none'}`,
    );
  }

  return files[0];
}

function sha256(path) {

  return createHash('sha256')
    .update(
      readFileSync(path),
    )
    .digest('hex');
}

function artifactRecord(
  role,
  path,
) {

  return {
    role,

    name:
      basename(path),

    sha256:
      sha256(path),

    size:
      statSync(path).size,
  };
}

function makeExecutable(path) {

  if (
    descriptor.os !== 'win32'
  ) {

    chmodSync(
      path,
      statSync(path).mode |
      0o111,
    );
  }
}

/* ==========================================================
 * DEBUG-ONLY PRE-PUBLICATION SETUP HARNESS
 * ========================================================== */

function buildDebugSetupCli() {

  const cargoTarget =
    resolve(
      repo,
      'target',
      'fileflow-e2e',
    );

  run(
    'cargo',
    [
      'build',
      '-p',
      'fileflow-setup',
      '--bin',
      'fileflow-setup-cli',
      '--target',
      target,
    ],
    {
      env: {
        CARGO_TARGET_DIR:
          cargoTarget,
      },
    },
  );

  const executable =
    join(
      cargoTarget,
      target,
      'debug',

      descriptor.os === 'win32'
        ? 'fileflow-setup-cli.exe'
        : 'fileflow-setup-cli',
    );

  if (!existsSync(executable)) {

    throw new Error(
      `debug Setup CLI missing: ` +
      `${executable}`,
    );
  }

  makeExecutable(executable);

  return executable;
}

/* ==========================================================
 * PREVIOUS STABLE RELEASE
 * ========================================================== */

async function githubJson(url) {

  const headers = {
    Accept:
      'application/vnd.github+json',

    'User-Agent':
      'FileFlow-native-e2e',

    'X-GitHub-Api-Version':
      '2022-11-28',
  };

  const token =
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN;

  if (token) {
    headers.Authorization =
      `Bearer ${token}`;
  }

  const response =
    await fetch(
      url,
      {
        headers,
      },
    );

  if (!response.ok) {

    throw new Error(
      `GitHub request failed ` +
      `${response.status}: ${url}`,
    );
  }

  return response.json();
}

async function downloadPreviousStable() {

  const releases =
    await githubJson(
      'https://api.github.com/repos/' +
      'idris-ach2002/FileFlow/releases' +
      '?per_page=40',
    );

  const candidates =
    releases

      .filter(
        (release) => (
          !release.draft &&
          !release.prerelease
        ),
      )

      .map((release) => ({
        release,

        version:
          String(
            release.tag_name || '',
          ).replace(/^v/, ''),
      }))

      .filter(
        (entry) => (
          /^\d+\.\d+\.\d+$/.test(
            entry.version,
          ) &&
          compareVersion(
            entry.version,
            version,
          ) < 0
        ),
      )

      .sort(
        (left, right) =>
          compareVersion(
            right.version,
            left.version,
          ),
      );

  const previousRelease =
    candidates[0];

  if (!previousRelease) {

    throw new Error(
      `no stable N-1 release ` +
      `lower than ${version}`,
    );
  }

  const manifestAsset =
    previousRelease
      .release
      .assets
      ?.find(
        (asset) =>
          asset.name ===
          'downloads.json',
      );

  if (!manifestAsset) {

    throw new Error(
      `${previousRelease.release.tag_name} ` +
      `has no downloads.json asset`,
    );
  }

  const headers = {
    'User-Agent':
      'FileFlow-native-e2e',
  };

  const manifestResponse =
    await fetch(
      manifestAsset
        .browser_download_url,
      {
        headers,
      },
    );

  if (!manifestResponse.ok) {

    throw new Error(
      `cannot download previous ` +
      `downloads.json: HTTP ` +
      `${manifestResponse.status}`,
    );
  }

  const manifest =
    await manifestResponse.json();

  if (
    manifest.version !==
    previousRelease.version
  ) {

    throw new Error(
      'previous downloads.json ' +
      'version mismatch',
    );
  }

  const artifact =
    manifest
      .platforms
      ?.[descriptor.key]
      ?.application;

  if (
    !artifact?.url ||
    !artifact?.sha256 ||
    !artifact?.name
  ) {

    throw new Error(
      `previous application ` +
      `missing for ${descriptor.key}`,
    );
  }

  const directory =
    resolve(
      repo,
      'dist',
      'e2e',
      target,
      'previous',
    );

  mkdirSync(
    directory,
    {
      recursive: true,
    },
  );

  const path =
    join(
      directory,
      artifact.name,
    );

  const response =
    await fetch(
      artifact.url,
      {
        headers,
      },
    );

  if (!response.ok) {

    throw new Error(
      `previous application ` +
      `download failed: HTTP ` +
      `${response.status}`,
    );
  }

  const bytes =
    Buffer.from(
      await response.arrayBuffer(),
    );

  writeFileSync(
    path,
    bytes,
  );

  if (
    artifact.size &&
    bytes.length !== artifact.size
  ) {

    throw new Error(
      'previous application ' +
      'size mismatch',
    );
  }

  if (
    sha256(path)
      .toLowerCase() !==
    String(
      artifact.sha256,
    ).toLowerCase()
  ) {

    throw new Error(
      'previous application ' +
      'SHA-256 mismatch',
    );
  }

  makeExecutable(path);

  return {
    version:
      previousRelease.version,

    path,
  };
}

function compareVersion(
  left,
  right,
) {

  const a =
    left
      .split('.')
      .map(Number);

  const b =
    right
      .split('.')
      .map(Number);

  for (
    let index = 0;
    index < 3;
    index += 1
  ) {

    if (
      a[index] !== b[index]
    ) {

      return (
        a[index] -
        b[index]
      );
    }
  }

  return 0;
}

/* ==========================================================
 * SETUP INVOCATION
 * ========================================================== */

function runLocalSetup(
  cli,
  artifact,
  artifactVersion,
  cliArgs,
) {

  run(
    cli,
    cliArgs,
    {
      env: {
        FILEFLOW_SETUP_LOCAL_APPLICATION:
          artifact,

        FILEFLOW_SETUP_LOCAL_VERSION:
          artifactVersion,

        RUST_BACKTRACE:
          '1',
      },
    },
  );
}

/* ==========================================================
 * SYSTEM STATE
 * ========================================================== */

function receiptPath() {

  if (
    descriptor.os === 'darwin'
  ) {

    return join(
      homedir(),
      'Library',
      'Application Support',
      'FileFlow',
      'install-receipt.json',
    );
  }

  if (
    descriptor.os === 'linux'
  ) {

    return join(
      process.env.XDG_DATA_HOME ||
        join(
          homedir(),
          '.local',
          'share',
        ),

      'fileflow',
      'install-receipt.json',
    );
  }

  return join(
    process.env.LOCALAPPDATA ||
      join(
        homedir(),
        'AppData',
        'Local',
      ),

    'FileFlow',
    'install-receipt.json',
  );
}

function readReceipt() {

  const path =
    receiptPath();

  if (!existsSync(path)) {

    throw new Error(
      `install receipt missing: ${path}`,
    );
  }

  return JSON.parse(
    readFileSync(
      path,
      'utf8',
    ),
  );
}

function applicationCandidates() {

  if (
    descriptor.os === 'darwin'
  ) {

    return [
      '/Applications/FileFlow.app',

      join(
        homedir(),
        'Applications',
        'FileFlow.app',
      ),
    ];
  }

  if (
    descriptor.os === 'linux'
  ) {

    return [
      join(
        homedir(),
        '.local',
        'opt',
        'fileflow',
        'FileFlow.AppImage',
      ),
    ];
  }

  const local =
    process.env.LOCALAPPDATA ||
    join(
      homedir(),
      'AppData',
      'Local',
    );

  const programFiles =
    process.env.ProgramFiles ||
    'C:\\Program Files';

  return [
    join(
      local,
      'Programs',
      'FileFlow',
      'FileFlow.exe',
    ),

    join(
      local,
      'FileFlow',
      'FileFlow.exe',
    ),

    join(
      programFiles,
      'FileFlow',
      'FileFlow.exe',
    ),
  ];
}

function integrationPaths() {

  if (
    descriptor.os === 'darwin'
  ) {
    return [];
  }

  if (
    descriptor.os === 'linux'
  ) {

    return [
      join(
        homedir(),
        '.local',
        'share',
        'applications',
        'fileflow.desktop',
      ),
    ];
  }

  const appdata =
    process.env.APPDATA ||
    join(
      homedir(),
      'AppData',
      'Roaming',
    );

  return [
    join(
      appdata,
      'Microsoft',
      'Windows',
      'Start Menu',
      'Programs',
      'FileFlow.lnk',
    ),

    join(
      appdata,
      'Microsoft',
      'Windows',
      'Start Menu',
      'Programs',
      'FileFlow',
      'FileFlow.lnk',
    ),
  ];
}

function verifyInstalled(
  expectedVersion,
) {

  const application =
    applicationCandidates()
      .find(existsSync);

  if (!application) {

    throw new Error(
      `FileFlow application is ` +
      `absent after operation ` +
      `(${descriptor.key})`,
    );
  }

  if (
    descriptor.os !== 'darwin'
  ) {

    const integration =
      integrationPaths()
        .find(existsSync);

    if (!integration) {

      throw new Error(
        'FileFlow launcher ' +
        'integration is absent',
      );
    }
  }

  const receipt =
    readReceipt();

  if (
    receipt.applicationVersion !==
    expectedVersion
  ) {

    throw new Error(
      `receipt version mismatch: ` +
      `expected ${expectedVersion}, ` +
      `got ${receipt.applicationVersion}`,
    );
  }

  console.log(
    `[e2e] installed ` +
    `${expectedVersion}: ` +
    `${application}`,
  );
}

function verifyClean() {

  const leftovers = [
    ...applicationCandidates(),
    ...integrationPaths(),
  ].filter(existsSync);

  if (leftovers.length) {

    throw new Error(
      `uninstall leftovers: ` +
      `${leftovers.join(', ')}`,
    );
  }

  console.log(
    '[e2e] uninstall state clean',
  );
}

/* ==========================================================
 * EPHEMERAL RUNNER CLEANUP
 * ========================================================== */

function cleanMachine() {

  console.log(
    '[e2e] cleaning ephemeral runner state',
  );

  if (
    descriptor.os === 'win32'
  ) {

    run(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',

        `& {
          Get-Process -Name FileFlow,fileflow-desktop -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue

          $paths=@(
            "$env:LOCALAPPDATA\\Programs\\FileFlow",
            "$env:LOCALAPPDATA\\FileFlow",
            "$env:ProgramFiles\\FileFlow",

            "$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\FileFlow.lnk",
            "$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\\FileFlow",

            "$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\FileFlow.lnk",
            "$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\FileFlow"
          )

          foreach($path in $paths) {
            if(Test-Path -LiteralPath $path) {
              Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
            }
          }
        }`,
      ],
    );

    return;
  }

  run(
    'pkill',
    [
      '-f',
      'fileflow-desktop',
    ],
    {
      accepted: [
        0,
        1,
      ],
    },
  );

  if (
    descriptor.os === 'darwin'
  ) {

    for (
      const path of [
        '/Applications/FileFlow.app',

        join(
          homedir(),
          'Applications',
          'FileFlow.app',
        ),

        join(
          homedir(),
          'Library',
          'Application Support',
          'FileFlow',
        ),
      ]
    ) {

      rmSync(
        path,
        {
          recursive: true,
          force: true,
        },
      );
    }

    return;
  }

  for (
    const path of [
      join(
        homedir(),
        '.local',
        'opt',
        'fileflow',
      ),

      join(
        homedir(),
        '.local',
        'bin',
        'fileflow',
      ),

      join(
        homedir(),
        '.local',
        'share',
        'applications',
        'fileflow.desktop',
      ),

      join(
        homedir(),
        '.local',
        'share',
        'fileflow',
      ),
    ]
  ) {

    rmSync(
      path,
      {
        recursive: true,
        force: true,
      },
    );
  }

  run(
    'bash',
    [
      '-lc',

      'rm -f ' +
      '"$HOME/.local/share/icons/hicolor/"' +
      '*/apps/fileflow.png',
    ],
  );
}

/* ==========================================================
 * DELIBERATE DAMAGE
 * ========================================================== */

function damageIntegration() {

  if (
    descriptor.os === 'darwin'
  ) {

    const application =
      applicationCandidates()
        .find(existsSync);

    if (!application) {

      throw new Error(
        'cannot damage missing ' +
        'macOS application',
      );
    }

    const backup =
      `${application}.e2e-broken`;

    rmSync(
      backup,
      {
        recursive: true,
        force: true,
      },
    );

    renameSync(
      application,
      backup,
    );

    console.log(
      `[e2e] deliberately removed ` +
      `canonical macOS app -> ` +
      `${backup}`,
    );

    return {
      backup,
    };
  }

  const integration =
    integrationPaths()
      .find(existsSync);

  if (!integration) {

    throw new Error(
      'cannot damage missing ' +
      'integration',
    );
  }

  rmSync(
    integration,
    {
      recursive: true,
      force: true,
    },
  );

  console.log(
    `[e2e] deliberately removed ` +
    `integration -> ${integration}`,
  );

  return {
    integration,
  };
}

/* ==========================================================
 * WINDOWS MSI
 * ========================================================== */

function testWindowsMsiPackages() {

  const appMsi =
    selectOne(
      mainBundle,

      (path) => (
        path
          .toLowerCase()
          .endsWith('.msi') &&

        !basename(path)
          .toLowerCase()
          .includes(
            'fileflowsetup',
          )
      ),

      'application MSI',
    );

  const setupMsi =
    selectOne(
      setupBundle,

      (path) =>
        path
          .toLowerCase()
          .endsWith('.msi'),

      'Setup MSI',
    );

  for (
    const [
      role,
      msi,
    ] of [
      [
        'application-msi',
        appMsi,
      ],
      [
        'setup-msi',
        setupMsi,
      ],
    ]
  ) {

    testedArtifacts.push(
      artifactRecord(
        role,
        msi,
      ),
    );

    run(
      'msiexec.exe',
      [
        '/i',
        msi,
        '/qn',
        '/norestart',
      ],
      {
        accepted: [
          0,
          1641,
          3010,
        ],
      },
    );

    if (
      role ===
      'application-msi'
    ) {

      const app =
        applicationCandidates()
          .find(existsSync);

      if (!app) {

        throw new Error(
          'MSI returned success but ' +
          'FileFlow is not installed',
        );
      }
    }

    run(
      'msiexec.exe',
      [
        '/x',
        msi,
        '/qn',
        '/norestart',
      ],
      {
        accepted: [
          0,
          1641,
          3010,
        ],
      },
    );
  }

  cleanMachine();
}

/* ==========================================================
 * LINUX DEB
 * ========================================================== */

function testLinuxDebPackages() {

  const appDeb =
    selectOne(
      mainBundle,

      (path) =>
        path
          .toLowerCase()
          .endsWith('.deb'),

      'application DEB',
    );

  const setupDeb =
    selectOne(
      setupBundle,

      (path) =>
        path
          .toLowerCase()
          .endsWith('.deb'),

      'Setup DEB',
    );

  for (
    const [
      role,
      deb,
    ] of [
      [
        'application-deb',
        appDeb,
      ],
      [
        'setup-deb',
        setupDeb,
      ],
    ]
  ) {

    testedArtifacts.push(
      artifactRecord(
        role,
        deb,
      ),
    );

    const packageName =
      capture(
        'dpkg-deb',
        [
          '-f',
          deb,
          'Package',
        ],
      ).trim();

    if (
      !/^[A-Za-z0-9.+-]+$/.test(
        packageName,
      )
    ) {
      throw new Error(
        `unsafe DEB package name: ` +
        `${packageName}`,
      );
    }

    const install =
      spawnSync(
        'sudo',
        [
          'dpkg',
          '-i',
          deb,
        ],
        {
          cwd: repo,
          env: process.env,
          encoding: 'utf8',
          stdio: 'inherit',
        },
      );

    if (install.error) {
      throw install.error;
    }

    if (
      install.status !== 0
    ) {

      run(
        'sudo',
        [
          'apt-get',
          'install',
          '-f',
          '-y',
        ],
      );
    }

    const status =
      capture(
        'dpkg-query',
        [
          '-W',
          '-f=${Status}',
          packageName,
        ],
      ).trim();

    if (
      status !==
      'install ok installed'
    ) {

      throw new Error(
        `DEB package not installed: ` +
        `${packageName} (${status})`,
      );
    }

    run(
      'sudo',
      [
        'dpkg',
        '-r',
        packageName,
      ],
    );
  }

  cleanMachine();
}

/* ==========================================================
 * LINUX RPM — REAL FEDORA USERLAND
 * ========================================================== */

function testLinuxRpmPackages() {

  const appRpm =
    selectOne(
      mainBundle,

      (path) =>
        path
          .toLowerCase()
          .endsWith('.rpm'),

      'application RPM',
    );

  const setupRpm =
    selectOne(
      setupBundle,

      (path) =>
        path
          .toLowerCase()
          .endsWith('.rpm'),

      'Setup RPM',
    );

  for (
    const [
      role,
      rpm,
    ] of [
      [
        'application-rpm',
        appRpm,
      ],
      [
        'setup-rpm',
        setupRpm,
      ],
    ]
  ) {

    testedArtifacts.push(
      artifactRecord(
        role,
        rpm,
      ),
    );

    const packageName =
      capture(
        'rpm',
        [
          '-qp',
          '--qf',
          '%{NAME}',
          rpm,
        ],
      ).trim();

    if (
      !/^[A-Za-z0-9_.+:-]+$/.test(
        packageName,
      )
    ) {

      throw new Error(
        `unsafe RPM package name: ` +
        `${packageName}`,
      );
    }

    run(
      'docker',
      [
        'run',
        '--rm',

        '-v',
        `${rpm}:/tmp/fileflow.rpm:ro`,

        'fedora:42',

        'bash',
        '-lc',

        (
          'dnf install -y ' +
          '/tmp/fileflow.rpm' +
          ' && rpm -q ' +
          packageName +
          ' && dnf remove -y ' +
          packageName
        ),
      ],
    );
  }
}
