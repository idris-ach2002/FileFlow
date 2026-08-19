# FileFlow

FileFlow is a cross-platform desktop application for local file conversion, organisation and automation. The initial desktop targets are **macOS** and **Linux**.

## Architecture

- **Angular 22**: desktop UI only.
- **Tauri 2**: native desktop shell and IPC boundary.
- **Rust core**: domain model, workspaces, planning, scheduling, execution and output safety.
- **Tokio**: async I/O, process orchestration and queues.
- **Rayon**: CPU-bound parallel work owned by FileFlow.
- **SQLite / rusqlite**: settings, recipes and history.
- **External engines**: FFmpeg, libvips, ImageMagick, qpdf, Poppler, Ghostscript, LibreOffice, OCRmyPDF/Tesseract, Pandoc, 7-Zip and ExifTool.

External engines are discovered at runtime. Missing engines disable only their related capabilities; they do not prevent FileFlow from starting.

## Repository layout

```text
frontend/                 Angular UI
src-tauri/                Tauri application shell / IPC / platform bundling
crates/fileflow-domain/   Shared domain types and resource profiles
crates/fileflow-engine/   Engine abstraction and cross-platform executable probing
crates/fileflow-core/     Engine registry and application core
crates/fileflow-intake/   File/folder/archive intake
crates/fileflow-formats/  Format registry and MIME/magic detection
crates/fileflow-workspace Workspace model and grouping
crates/fileflow-planner/  Transformation planning / DAG
crates/fileflow-scheduler Resource-aware scheduling
crates/fileflow-executor/ Job execution / cancellation
crates/fileflow-output/   Atomic outputs / naming / conflict handling
crates/fileflow-storage/  SQLite persistence
crates/adapters/          Conversion engine adapters
scripts/                  Portable setup and diagnostics
```

## Toolchain

Angular 22 requires a supported Node version. The repository pins Node `22.22.3` in `.nvmrc` as the recommended development version.

Rust is managed with `rust-toolchain.toml` and uses the stable toolchain.

## First setup: macOS or Linux

Use the platform-aware setup entry point:

```bash
sh scripts/setup.sh
```

It detects the host automatically:

- macOS -> `scripts/setup-macos.sh`
- Linux -> `scripts/setup-linux.sh`

Then bootstrap JavaScript and Rust dependencies:

```bash
sh scripts/bootstrap.sh
```

Check detected conversion engines at any time:

```bash
npm run engines
```

Run the desktop application:

```bash
pnpm run dev
```

### macOS notes

Tauri desktop development on macOS requires Xcode Command Line Tools. `scripts/setup-macos.sh` checks this and can start Apple's installer when they are missing.

The conversion engines are installed through Homebrew. LibreOffice is installed as the macOS application bundle and FileFlow also probes `/Applications/LibreOffice.app/Contents/MacOS/soffice` directly.

FileFlow does not rely only on the shell `PATH` when probing conversion engines. This matters for an application started from Finder or the Dock: the engine probe also checks the standard Apple Silicon Homebrew prefix `/opt/homebrew/bin` and the Intel prefix `/usr/local/bin`.

Build a native package for the current Mac architecture:

```bash
pnpm run build:mac
```

This produces the macOS application bundle and DMG through Tauri.

To build a universal macOS binary for both Apple Silicon and Intel, install both Rust targets once:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
ppnpm run build:mac:universal
```

Signing/notarization is a distribution concern and will be configured when FileFlow is ready to ship to other Mac users.

### Debian / Ubuntu

The portable entry point works here too:

```bash
sh scripts/setup.sh
```

Or call the Linux helper directly:

```bash
sh scripts/setup-linux.sh
```

Build Linux packages:

```bash
pnpm run build:linux
```

The Linux Tauri configuration currently targets `.deb` and AppImage.

## Platform-specific Tauri configuration

Tauri automatically merges the matching platform file:

```text
src-tauri/tauri.conf.json          common configuration
src-tauri/tauri.macos.conf.json   .app / .dmg and macOS settings
src-tauri/tauri.linux.conf.json   .deb / AppImage settings
```

This keeps the application runtime architecture shared while allowing native packaging on each operating system.

## Quality commands

```bash
npm run frontend:build
npm run check:rust
npm run test:rust
npm run clippy
npm run fmt
npm run check
npm run test
```

## Git

No Git repository is included intentionally. Initialise it after validating the local toolchain:

```bash
git init
git add .
git commit -m "chore: initialize FileFlow architecture"
```

Then return the complete project including `.git` so future work can be performed as atomic commits.

## Upstream references

- Tauri prerequisites: https://v2.tauri.app/start/prerequisites/
- Tauri macOS bundle: https://v2.tauri.app/distribute/macos-application-bundle/
- Tauri DMG: https://v2.tauri.app/distribute/dmg/
- Angular version compatibility: https://angular.dev/reference/versions
