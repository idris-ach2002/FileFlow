# FileFlow native engine packs

A release pack is target-specific and lives outside Git at:

```
release/engines/packs/<target-triple>/
├── bin/       # executables + DLL/helper binaries
├── lib/       # dylib/so dependencies when needed
├── share/     # runtime data (for example tessdata)
└── licenses/  # one `<engine-id>.txt` notice per redistributed engine
```

`python scripts/release/stage-engines.py --target <triple> --mode <mode>` copies the pack into the Tauri resources directory and validates the executable contract in `manifest.json`.

## Modes

- `optional`: no bundled engine is mandatory. Useful for development and packaging smoke tests; FileFlow falls back to system executables.
- `core`: all `tier=core` engines must be present. This is the intended minimum self-contained public edition.
- `full`: every manifest engine must be present. Do not publish this flavor until redistribution licenses and dynamic dependencies have been audited for every target.

## Runtime lookup order

1. FileFlow bundled pack (`resources/engines/bin`)
2. process `PATH`
3. known platform package-manager locations

Bundled engines therefore make a release deterministic while preserving a development fallback.

## Target triples

Current CI targets:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Do not copy a Homebrew/Linux package tree wholesale into these directories. Packs must be self-contained for the target and each redistributed component must have a license notice.

## CI retrieval

For `core` and `full` releases, set GitHub variable `FILEFLOW_ENGINE_PACK_URL_TEMPLATE` to an HTTPS template containing `{target}`, for example:

```text
https://downloads.example.com/fileflow/v1/engines/fileflow-engines-{target}.tar.gz
```

The CI downloads both the archive and `<archive>.sha256`, verifies SHA-256, rejects path traversal/symlink entries, then stages the pack. `optional` builds do not require a remote pack.

A prepared local pack can be archived with:

```bash
pnpm run release:pack-engines -- --target aarch64-apple-darwin
```
