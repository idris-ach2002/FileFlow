# FileFlow architecture

## Supported desktop targets

The core architecture is platform-independent. The initial native desktop targets are:

- macOS (Apple Silicon and Intel)
- Linux (Debian/Ubuntu packaging first)

Platform-specific behavior must stay behind the Tauri shell, filesystem/platform services or engine-discovery layer. Conversion/planning/domain crates must not depend on a specific desktop OS.

## Runtime pipeline

```text
Angular UI
   │
   │ Tauri commands / channels
   ▼
Tauri shell
   │
   ├── macOS integration
   ├── Linux integration
   │
   ▼
FileFlow Core
   ├── Intake
   ├── Format Registry
   ├── Workspace
   ├── Planner
   ├── Resource Scheduler
   ├── Executor
   └── Output Manager
          │
          ▼
      Engine adapters
          ├── FFmpeg
          ├── libvips
          ├── ImageMagick
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

## Engine discovery

Engine discovery does not assume that an installed desktop application inherits the interactive shell environment.

The probe order is:

1. adapter-specific known absolute paths;
2. the current process `PATH`;
3. standard platform executable directories.

On macOS this includes both common Homebrew prefixes and the standard LibreOffice `.app` location. This is required so FileFlow behaves the same when launched through `npm run dev`, Finder, Dock or an installed DMG.

## Concurrency model

- Tauri commands do not perform expensive work on the UI thread.
- Tokio owns asynchronous orchestration, child processes, filesystem I/O, queues and cancellation.
- Rayon owns FileFlow CPU-bound parallel algorithms such as hashing and duplicate analysis.
- External engines may already be internally multithreaded; the scheduler must account for that instead of blindly launching one engine per CPU.
- Every engine advertises a preliminary `ResourceProfile` (CPU, RAM, I/O, internal threading, max concurrent instances).
- UI progress events are throttled/aggregated before crossing IPC.

## Packaging model

```text
shared source tree
      │
      ├── macOS build
      │      ├── FileFlow.app
      │      └── FileFlow.dmg
      │
      └── Linux build
             ├── FileFlow.deb
             └── FileFlow.AppImage
```

Docker is intentionally not part of the runtime or initial development workflow.

## Safety principles

1. Originals are never overwritten by default.
2. Jobs write to a private temporary workspace and atomically finalize outputs.
3. Archive extraction must prevent path traversal and zip bombs.
4. External commands are invoked directly with argument arrays, never through `sh -c`.
5. Missing engines reduce capabilities instead of crashing startup.
6. Platform-specific filesystem and packaging details do not leak into the domain/core model.
