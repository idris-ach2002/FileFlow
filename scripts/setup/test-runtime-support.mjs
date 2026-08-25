#!/usr/bin/env node

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const unix = readFileSync('scripts/runtime/install-dependencies.sh', 'utf8');
assert.match(unix, /sudo -v/);
assert.match(unix, /sudo -n "\$@"/);
assert.match(unix, /Session administrateur validée/);
assert.doesNotMatch(unix, /elif has sudo; then\s*\n\s*sudo "\$@"/);

const windows = readFileSync('scripts/runtime/install-dependencies.ps1', 'utf8');
assert.match(windows, /Ensure-GitBashSupport/);
assert.match(windows, /winget:Git\.Git/);
assert.match(windows, /support:git-bash/);
assert.match(windows, /-Kind 'integration'/);
assert.ok([...windows].every((character) => character.codePointAt(0) <= 127),
  'install-dependencies.ps1 must remain ASCII for Windows PowerShell 5.1');

const setupUi = readFileSync('setup-ui/index.html', 'utf8');
const setupScript = readFileSync('setup-ui/setup.js', 'utf8');
const adapter = readFileSync('setup-tauri/src/adapter.rs', 'utf8');
const cli = readFileSync('setup-tauri/src/cli.rs', 'utf8');
assert.match(setupUi, /remove-preexisting-engines/);
assert.match(setupUi, /resource-status/);
assert.match(setupScript, /removePreexistingEngines/);
assert.match(setupScript, /resource-progress/);
assert.match(setupScript, /engine\.installed && !engine\.installedByFileflow/);
assert.match(adapter, /let owned = if plan\.request\.remove_owned_engines/);
assert.match(cli, /--remove-preexisting-engines exige --engines id,id/);

console.log('[setup-support] privilege session, Windows support tool and expert UI wiring verified');
