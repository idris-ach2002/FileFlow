import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(
  new URL('../../setup-tauri/src/adapter.rs', import.meta.url),
  'utf8',
);

assert.match(
  source,
  /fn windows_powershell_program\(\)/,
  'PowerShell runner selection is required',
);

assert.match(
  source,
  /FILEFLOW_PS_PATH/,
  'Authenticode path must use environment binding',
);

assert.match(
  source,
  /Import-Module Microsoft\.PowerShell\.Security -ErrorAction Stop/,
  'Authenticode security module must load explicitly',
);

assert.match(
  source,
  /FILEFLOW_PS_TARGET[\s\S]{0,500}FILEFLOW_PS_SHORTCUT[\s\S]{0,500}FILEFLOW_PS_WORKING_DIRECTORY/,
  'shortcut paths must use environment binding',
);

assert.doesNotMatch(
  source,
  /param\(\[string\]\$Path\)/,
  'positional Authenticode binding must not return',
);

assert.doesNotMatch(
  source,
  /Remove-Item -LiteralPath \$args\[0\]/,
  'maintenance removal must not use positional path binding',
);

console.log(
  '[windows-powershell-source] safe PowerShell path binding verified',
);
