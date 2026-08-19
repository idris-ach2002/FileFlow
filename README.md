# FileFlow

FileFlow is a local-first desktop application for file conversion, organisation and automation. It is designed to expose powerful file tooling through a simple interface that remains usable by non-technical users.

Initial desktop targets: **macOS** and **Linux**.

## Current capabilities

The repository already contains a working native desktop foundation, not only UI mockups:

- native file/folder pickers and Finder/Linux drag & drop;
- recursive intake with bounded batches, backpressure, hidden/symlink safeguards and paginated workspaces;
- magic/extension format detection and grouping by file family;
- server-side workspace search, filters, sorting, insights and recommendations;
- conversion capability graph with multi-step route planning;
- resource-aware scheduler with CPU/RAM/I/O and per-engine quotas;
- cancellable external processes invoked directly without a shell;
- safe temporary outputs, conflict handling and atomic finalisation;
- local SQLite history, favourites and recipes;
- exact duplicate confirmation using staged SHA-256 hashing;
- responsive Angular desktop UI, command palette, dark/light themes and native tray navigation;
- secure post-conversion actions: open, reveal in Finder/file manager and save a copy.

### Locally executable actions

The current runtime wires these actions to real local engines:

- images -> PDF;
- image conversion and batch conversion;
- image optimisation and resizing;
- metadata removal and metadata extraction;
- Office/OpenDocument -> PDF;
- PDF merge, split, compression, OCR, text extraction and PDF -> images;
- image OCR;
- archive creation and guarded extraction;
- media compatibility conversion and compression;
- audio conversion and audio extraction;
- video -> GIF.

Additional actions are already represented by the capability planner and can be connected without changing the UI/domain architecture.

## Architecture

- **Angular 22**: desktop UI only.
- **Tauri 2**: native desktop shell, IPC, tray and platform integration.
- **Rust core**: domain model, workspaces, planning, scheduling, execution and output safety.
- **Tokio**: async I/O, child-process orchestration, bounded queues and cancellation.
- **Bounded CPU workers**: hashing/analysis work is deliberately isolated from the async runtime.
- **SQLite / rusqlite**: settings, favourites, recipes and history.
- **External engines**: FFmpeg, libvips, ImageMagick, img2pdf, qpdf, Poppler, Ghostscript, LibreOffice, OCRmyPDF/Tesseract, Pandoc, 7-Zip and ExifTool.

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

## First setup: macOS or Linux

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

## Linux

```bash
sh scripts/setup-linux.sh
pnpm run build:linux
```

The Linux configuration currently targets `.deb` and AppImage.

## Tauri platform configuration

```text
src-tauri/tauri.conf.json          shared configuration
src-tauri/tauri.macos.conf.json   macOS app/DMG settings
src-tauri/tauri.linux.conf.json   Linux deb/AppImage settings
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
