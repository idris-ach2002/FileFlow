#!/usr/bin/env node
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

if (process.platform === 'win32') {
  console.log('[setup-ownership] test shell couvert par les tests PowerShell/Rust sous Windows');
  process.exit(0);
}

const root = resolve(import.meta.dirname, '../..');
const temporary = mkdtempSync(join(tmpdir(), 'fileflow-engine-ownership-'));
const bin = join(temporary, 'bin');
const report = join(temporary, 'installed.tsv');

try {
  mkdirSync(bin, { recursive: true });
  executable('uname', '#!/bin/sh\n[ "$1" = "-m" ] && echo x86_64 || echo Linux\n');
  executable('id', '#!/bin/sh\necho 0\n');
  executable('mkdir', '#!/bin/sh\nexec /bin/mkdir "$@"\n');
  executable('dirname', '#!/bin/sh\nexec /usr/bin/dirname "$@"\n');
  executable('chmod', '#!/bin/sh\nexec /bin/chmod "$@"\n');
  executable('dpkg-query', '#!/bin/sh\nexit 1\n');
  executable('apt-get', `#!/bin/sh
case " $* " in
  *" install -y qpdf "*) printf '#!/bin/sh\\nexit 0\\n' > '${join(bin, 'qpdf')}'; chmod +x '${join(bin, 'qpdf')}' ;;
esac
exit 0
`);

  const result = spawnSync('/bin/bash', [
    resolve(root, 'scripts/runtime/install-dependencies.sh'),
    '--no-update', '--quiet', '--engines', 'qpdf', '--report', report,
  ], {
    cwd: root,
    env: {
      ...process.env,
      HOME: temporary,
      PATH: bin,
      FILEFLOW_SETUP_INSTALL_APP_RUNTIME: '0',
    },
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(readFileSync(report, 'utf8').trim(), 'qpdf\tapt\tqpdf\tengine');
  console.log('[setup-ownership] paquet exact enregistré sans modifier la machine de test');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}

function executable(name, source) {
  const path = join(bin, name);
  writeFileSync(path, source);
  chmodSync(path, 0o755);
}
