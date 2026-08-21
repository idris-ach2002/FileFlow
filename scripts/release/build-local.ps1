$ErrorActionPreference = 'Stop'
$Target = (rustc --print host-tuple 2>$null)
if (-not $Target) { $Target = ((rustc -vV) | Select-String '^host: ').ToString().Replace('host: ','') }
python scripts/release/generate-release-config.py --target $Target
pnpm exec tauri build --target $Target --config src-tauri/tauri.release.conf.json @args
