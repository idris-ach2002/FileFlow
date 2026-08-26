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
const probe = readFileSync('crates/fileflow-setup-core/src/probe.rs', 'utf8');
const setupConfig = readFileSync('setup-tauri/tauri.conf.json', 'utf8');
const cli = readFileSync('setup-tauri/src/cli.rs', 'utf8');
assert.match(setupUi, /remove-preexisting-engines/);
assert.match(setupUi, /resource-status/);
assert.match(setupScript, /removePreexistingEngines/);
assert.match(setupScript, /resource-progress/);
assert.match(setupScript, /engine\.installed && !engine\.installedByFileflow/);
assert.match(adapter, /let owned = if plan\.request\.remove_owned_engines/);
assert.match(probe, /tasklist\.exe/);
assert.match(probe, /windows_tasklist_contains_fileflow/);
assert.match(probe, /fileflow-desktop\.exe/);
assert.match(adapter, /sanitize_appimage_environment/);
assert.match(adapter, /APPIMAGE_ORIGINAL_LD_LIBRARY_PATH/);
assert.match(adapter, /GIO_EXTRA_MODULES/);
assert.match(adapter, /"OWD"/);
assert.match(adapter, /Icon=fileflow/);
assert.match(adapter, /icon_sources/);
assert.match(
  adapter,
  /\$Target=\$env:FILEFLOW_PS_TARGET/,
);
assert.match(
  adapter,
  /\$Shortcut=\$env:FILEFLOW_PS_SHORTCUT/,
);
assert.match(
  adapter,
  /\$WorkingDirectory=\$env:FILEFLOW_PS_WORKING_DIRECTORY/,
);
assert.match(
  adapter,
  /IconLocation=\(\$Target \+ ',0'\)/,
);
assert.match(
  adapter,
  /Test-Path -LiteralPath \$Shortcut/,
);
assert.match(adapter, /FileFlow\.lnk/);
assert.match(setupConfig, /\.\.\/src-tauri\/icons\/icon\.png/);
assert.match(setupUi, /copy-diagnostic/);
assert.match(setupUi, /setup-update-action/);
assert.match(setupScript, /setup_update_status/);
assert.match(setupScript, /Raccourci \+ icône vérifiés/);
assert.match(cli, /--remove-preexisting-engines exige --engines id,id/);

console.log('[setup-support] privileges, locale-safe Windows process detection, AppImage isolation, desktop branding and diagnostic UX verified');
