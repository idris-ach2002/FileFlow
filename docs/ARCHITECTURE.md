# FileFlow architecture

## Design goal

FileFlow deliberately separates a simple user-facing desktop experience from a much richer local execution engine. The UI expresses intent; Rust owns filesystem access, capability planning, scheduling, execution, persistence and safety.

## Supported desktop targets

- macOS (Apple Silicon and Intel)
- Linux (Debian/Ubuntu packaging first)

Platform-specific integration stays behind Tauri or engine discovery. Domain/planner/execution crates do not depend on Angular.

## Runtime pipeline

```text
Angular 22 UI
   │
   │ typed Tauri commands + bounded Channels
   ▼
Tauri desktop shell
   ├── native dialogs
   ├── drag/drop
   ├── tray navigation
   ├── notifications/opener
   └── platform packaging
   │
   ▼
FileFlow Core
   ├── Intake ───────────── streaming / backpressure
   ├── Format Registry ─── magic + extension detection
   ├── Workspace ───────── server-side query / insights
   ├── Capability Graph ── actions + conversion routes
   ├── Planner ─────────── direct and multi-step plans
   ├── Resource Scheduler  CPU / RAM / I/O / engine quotas
   ├── Executor ────────── cancellable processes / bounded batch
   ├── Analysis ────────── staged duplicate hashing
   ├── Storage ─────────── SQLite WAL
   └── Output Manager ──── non-destructive atomic finalisation
          │
          ▼
      Engine adapters
          ├── FFmpeg
          ├── libvips
          ├── ImageMagick
          ├── img2pdf
          ├── qpdf
          ├── Poppler
          ├── Ghostscript
          ├── LibreOffice
          ├── OCRmyPDF
          ├── Tesseract
          ├── Pandoc
          ├── 7-Zip
          └── ExifTool
```

## Intake and workspace model

Intake never buffers an entire large folder before reporting progress. Assets are emitted in bounded batches while the authoritative workspace remains in Rust. Angular asks for pages (currently 200 items) with server-side filtering/search/sorting.

Default filesystem behaviour:

- recursive folders supported;
- symlinks detected but not followed;
- hidden entries can be excluded;
- bounded magic-byte sampling rather than whole-file reads;
- warnings do not abort an otherwise valid workspace.

A workspace may then expose recommendations, extension distribution, largest files and size-based duplicate candidates without pushing the full asset set through IPC.

## Capability model

The planner owns two related models:

1. **Actions**: user intents such as “compress PDF”, “remove metadata” or “make video compatible”.
2. **Conversion edges**: format-to-format transitions with engine, cost and lossiness.

This allows FileFlow to find a multi-step route when no direct converter exists, while the executor can independently declare which actions are already wired in the current build.

## Concurrency and resource control

Tokio is used for async filesystem/process orchestration, bounded queues and cancellation. CPU-heavy native analysis is isolated on bounded worker threads instead of occupying Tokio workers.

The scheduler exposes budgets for:

- CPU tokens;
- memory estimates;
- I/O pressure;
- maximum parallel instances per engine.

Heavy external programs are not blindly launched according to logical CPU count. When an engine supports internal threading (currently FFmpeg and libvips paths), the executor passes the CPU budget granted by the scheduler to reduce nested oversubscription.

## Execution lifecycle

```text
Action request
   ↓
Validate capability + engine availability
   ↓
Resolve compatible workspace assets
   ↓
Create JobId + CancellationToken
   ↓
Acquire resource lease(s)
   ↓
Write to temporary output
   ↓
Validate process success
   ↓
Atomic finalisation / conflict policy
   ↓
Register recent outputs
   ↓
Persist metadata-only history
```

External engines are launched with `tokio::process::Command`, `stdin` disabled and kill-on-drop semantics. No shell command is assembled from user-controlled paths.

## Output safety

`OutputPolicy` independently models:

- destination (same folder / subfolder / custom);
- preserved source tree;
- conflict strategy;
- naming strategy;
- original overwrite policy.

The default remains non-destructive. Temporary output filenames preserve the final extension so format-aware engines still know what to produce.

Post-conversion actions are also server-validated. The frontend sends a `JobId` and output index; Rust resolves that against a bounded registry of paths actually produced during the current session before opening, revealing or copying the file.

## Archive safety

Before automatic extraction, FileFlow rejects suspicious archives using conservative guards:

- parent traversal / absolute paths / drive-prefixed paths;
- symbolic and hard-link entries;
- more than 100,000 entries;
- more than 100 GiB declared unpacked data;
- extreme compression ratio for large payloads.

Extraction uses a temporary directory followed by final rename.

## Duplicate confirmation

Workspace insights initially group possible duplicates by size only and explicitly label them as candidates. Exact confirmation uses a staged pipeline:

1. same-size grouping;
2. SHA-256 over first/last chunks + size;
3. full SHA-256 only for credible candidates.

This avoids fully hashing every file in a large directory while still requiring a cryptographic full-file match before reporting a confirmed duplicate group.

## Persistence

SQLite runs in WAL mode and stores:

- history metadata;
- favourite actions;
- recipes;
- settings/schema metadata.

Document contents are never stored in SQLite.

## Engine discovery

Probe order is adapter-aware and cross-platform. In addition to `PATH`, macOS checks common Homebrew locations and the LibreOffice `.app` executable. Missing engines reduce the capability set instead of blocking application startup.

## Packaging

```text
shared source tree
      │
      ├── macOS → FileFlow.app / FileFlow.dmg
      └── Linux → FileFlow.deb / FileFlow.AppImage
```

Docker is intentionally absent from runtime and the normal developer workflow.

## Quality gate

`pnpm run verify` is the merge barrier:

1. Angular production build;
2. Angular tests;
3. `cargo fmt --check`;
4. `cargo check --workspace --locked`;
5. `cargo test --workspace --locked`;
6. Clippy on all targets/features with warnings treated as errors.
