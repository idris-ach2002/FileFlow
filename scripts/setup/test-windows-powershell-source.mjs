import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(
  new URL('../../setup-tauri/src/adapter.rs', import.meta.url),
  'utf8',
);

assert.match(
  source,
  /"& \{ param\(\[string\]\$Path\); \(Get-AuthenticodeSignature -LiteralPath \$Path\)\.Status\.ToString\(\) \}"/,
  'Authenticode doit être exécuté dans un scriptblock PowerShell',
);

assert.doesNotMatch(
  source,
  /"param\(\[string\]\$Path\); \(Get-AuthenticodeSignature/,
  'La forme PowerShell non enveloppée ne doit jamais revenir',
);

assert.match(
  source,
  /let script = r#"& \{ param\(\[string\]\$Target,\[string\]\$Shortcut,\[string\]\$WorkingDirectory\);/,
  'La création du raccourci doit utiliser un scriptblock PowerShell',
);

console.log(
  '[windows-powershell-source] Authenticode + shortcut argument binding verified',
);
