# JavaScript package manager

FileFlow uses **pnpm 11.20.0**, pinned by the root `packageManager` field.

Why:

- deterministic monorepo/workspace behavior;
- supported directly by Tauri;
- avoids depending on the npm Arborist peer-resolution implementation;
- one lockfile (`pnpm-lock.yaml`) for the root and Angular workspace.

Node is pinned in `.nvmrc`. `scripts/bootstrap.sh` enables Corepack and activates
the pinned pnpm version before installing dependencies.

Normal commands:

```sh
pnpm install
pnpm run dev
pnpm run check
pnpm run test
```
