# FileFlow

FileFlow is a local-first desktop application for file conversion, organisation and automation. It is designed to expose powerful file tooling through a simple interface that remains usable by non-technical users.

Certified desktop targets: **macOS (Apple Silicon + Intel), Windows x64, Linux x64 and Linux ARM64**.

## Current capabilities

The repository already contains a working native desktop foundation, not only UI mockups:

- native file/folder pickers and desktop drag & drop;
- recursive intake with bounded batches, backpressure, hidden/symlink safeguards and paginated workspaces;
- magic/extension format detection and grouping by file family;
- server-side workspace search, filters, sorting, insights and recommendations;
- conversion capability graph with multi-step route planning;
- resource-aware scheduler with CPU/RAM/I/O and per-engine quotas;
- cancellable external processes invoked directly without a shell;
- safe temporary outputs, conflict handling and atomic finalisation;
- local SQLite accounts/profiles, onboarding, preferences, history, favourites and recipes;
- exact duplicate confirmation using staged SHA-256 hashing;
- human-first guided onboarding, searchable help, responsive Angular desktop UI, command palette, dark/light themes, zoom/accessibility preferences and native tray navigation;
- secure post-conversion actions: open, reveal in Finder/file manager, save a copy and re-analyse extracted folders.

### Locally executable actions

The current runtime wires these actions to real local engines:

- images -> PDF;
- image conversion and batch conversion;
- image optimisation and resizing;
- metadata removal and metadata extraction;
- Office/OpenDocument -> PDF;
- PDF merge, split, compression, OCR, text extraction and PDF -> images;
- image OCR;
- archive inspection by file family, archive creation and guarded extraction;
- media compatibility conversion and compression;
- audio conversion and audio extraction;
- video -> GIF;
- Zstandard and LZ4 lossless compression/decompression, including one-click TAR.ZST/TAR.LZ4 packaging for lots/folders;
- additional video format conversion and light document/text conversion via Pandoc.

Additional actions are already represented by the capability planner and can be connected without changing the UI/domain architecture.

## Human-first first launch

The first launch intentionally avoids technical vocabulary. FileFlow guides the user through local account creation/login, a default FileFlow result folder, a few safety/comfort preferences and a short interactive explanation. Guided mode then presents goals such as **PDF & documents**, **Photos & images**, **Compress**, **Open & extract**, **Audio & video** and **Organize & clean** instead of asking the user to choose codecs or engines.

The local account is device-local in the current architecture: passwords are derived locally, sessions are opaque and held in memory, and credentials are not persisted in browser storage. See [`docs/IDENTITY_SECURITY.md`](docs/IDENTITY_SECURITY.md) for the security boundary and future connected-account design. See [`docs/FORMAT_SUPPORT.md`](docs/FORMAT_SUPPORT.md) for the recognized-vs-executable format matrix.

## Architecture

- **Angular 22**: desktop UI only.
- **Tauri 2**: native desktop shell, IPC, tray and platform integration.
- **Rust core**: domain model, workspaces, planning, scheduling, execution and output safety.
- **Tokio**: async I/O, child-process orchestration, bounded queues and cancellation.
- **Bounded CPU workers**: hashing/analysis work is deliberately isolated from the async runtime.
- **SQLite / rusqlite**: settings, favourites, recipes and history.
- **External engines**: FFmpeg, libvips, ImageMagick, img2pdf, qpdf, Poppler, Ghostscript, LibreOffice, OCRmyPDF/Tesseract, Pandoc, 7-Zip, Zstandard, LZ4 and ExifTool.

External engines are discovered at runtime. Missing engines disable only their related capabilities; they do not prevent FileFlow from starting.

## Repository layout

```text
frontend/                    Angular UI
src-tauri/                   Tauri application shell / IPC / platform integration
crates/fileflow-domain/      Shared domain model
crates/fileflow-engine/      Engine abstraction and executable probing
crates/fileflow-core/        Engine registry and application core
crates/fileflow-intake/      File/folder/archive intake
crates/fileflow-formats/     Format registry and MIME/magic detection
crates/fileflow-workspace/   Workspace model, querying and insights
crates/fileflow-planner/     Action catalog and conversion graph
crates/fileflow-scheduler/   Resource-aware scheduling
crates/fileflow-executor/    Real job execution / cancellation
crates/fileflow-output/      Atomic outputs / naming / conflict handling
crates/fileflow-analysis/    Duplicate analysis
crates/fileflow-storage/     SQLite persistence
crates/adapters/             Conversion-engine adapters
scripts/                     Portable setup, verification and diagnostics
```

## Toolchain

The repository pins Node `22.22.3` in `.nvmrc` and pnpm `11.20.0` in `package.json`.
Rust is pinned by `rust-toolchain.toml`.

## First setup

```bash
sh scripts/setup.sh
sh scripts/bootstrap.sh
```

Check conversion engines:

```bash
pnpm run engines
```

Run the desktop application:

```bash
pnpm run dev
```

Run the complete quality gate before a merge:

```bash
pnpm run verify
```

The verification gate performs the Angular production build/tests and Rust formatting/check/tests/Clippy with the lockfile enforced.

## macOS

The setup helper checks Xcode Command Line Tools and Homebrew engines. FileFlow probes both common Homebrew prefixes (`/opt/homebrew/bin` and `/usr/local/bin`) plus the LibreOffice application bundle, so engine discovery does not rely only on an interactive shell `PATH`.

Current-architecture package:

```bash
pnpm run build:mac
```

Universal Apple Silicon + Intel package (after installing both Rust targets):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm run build:mac:universal
```


## Windows

Install the pinned Node/pnpm and Rust toolchains, then:

```powershell
pnpm install --frozen-lockfile
pnpm run verify
pnpm run build:windows
```

Public installers are signed in the release workflow; local/package-smoke builds may remain unsigned.

## Linux

```bash
sh scripts/setup-linux.sh
pnpm run build:linux
```

The Linux configuration targets `.deb`, AppImage and `.rpm` packages.

## Tauri platform configuration

```text
src-tauri/tauri.conf.json          shared configuration
src-tauri/tauri.macos.conf.json   macOS app/DMG settings
src-tauri/tauri.windows.conf.json Windows NSIS/MSI settings
src-tauri/tauri.linux.conf.json   Linux DEB/AppImage/RPM settings
```

Docker is intentionally not part of the runtime or the normal development workflow.

## Safety rules

1. Originals are never overwritten by default.
2. Outputs are written to temporary paths and finalised only after successful processing.
3. Archives are preflighted for traversal, links, extreme entry counts, unpacked size and suspicious compression ratios.
4. External tools are invoked directly with argument arrays; FileFlow never uses `sh -c` for user paths.
5. Batch concurrency is bounded by the scheduler instead of launching one heavy process per CPU.
6. Symlinks are not followed by intake or post-output copy operations by default.
7. Opening/copying a result from the UI is restricted to paths registered as outputs of the current FileFlow session.
8. History stores operation metadata only, never document contents.
