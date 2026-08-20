$ErrorActionPreference = "Stop"
Set-Location (Resolve-Path "$PSScriptRoot/../..")

$Target = if ($args.Count -gt 0) {
  $args[0]
} else {
  (rustc --print host-tuple).Trim()
}

$Mode = if ($env:FILEFLOW_ENGINE_MODE) {
  $env:FILEFLOW_ENGINE_MODE
} else {
  "optional"
}

python scripts/release/stage-engines.py --target $Target --mode $Mode
python scripts/release/smoke-engines.py --mode $Mode
python scripts/release/generate-release-config.py --target $Target
corepack enable
pnpm install --frozen-lockfile
pnpm run verify

if ($Target -notlike "*windows-msvc") {
  throw "build-local.ps1 only builds Windows targets; got $Target"
}

pnpm tauri build --target $Target --bundles "nsis,msi" --config src-tauri/tauri.release.conf.json
