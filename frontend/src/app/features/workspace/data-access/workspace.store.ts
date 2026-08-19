import { computed, inject, Injectable, signal } from '@angular/core';
import { open } from '@tauri-apps/plugin-dialog';
import { TauriBridgeService } from '../../../core/ipc/tauri-bridge.service';
import {
  Asset,
  FormatFamily,
  IntakeStats,
  IntakeWarning,
  WorkspaceIntakeEvent,
  WorkspaceSnapshot,
} from '../../../core/ipc/tauri.models';

export type WorkspacePhase = 'idle' | 'scanning' | 'ready' | 'error';

const EMPTY_STATS: IntakeStats = {
  discovered: 0,
  files: 0,
  directories: 0,
  archives: 0,
  symlinks: 0,
  totalBytes: 0,
  warnings: 0,
};

const PREVIEW_LIMIT = 200;
const PAGE_SIZE = 200;

@Injectable({ providedIn: 'root' })
export class WorkspaceStore {
  private readonly bridge = inject(TauriBridgeService);

  readonly phase = signal<WorkspacePhase>('idle');
  readonly workspace = signal<WorkspaceSnapshot | null>(null);
  readonly activeWorkspaceId = signal<string | null>(null);
  readonly stats = signal<IntakeStats>({ ...EMPTY_STATS });
  readonly assets = signal<Asset[]>([]);
  readonly warnings = signal<IntakeWarning[]>([]);
  readonly error = signal<string | null>(null);
  readonly dragActive = signal(false);
  readonly pageTotal = signal(0);
  readonly familyFilter = signal<FormatFamily | null>(null);

  readonly busy = computed(() => this.phase() === 'scanning');
  readonly hasWorkspace = computed(() => this.workspace() !== null || this.activeWorkspaceId() !== null);
  readonly hasMore = computed(() => this.assets().length < this.pageTotal());
  readonly counts = computed(() => {
    const workspace = this.workspace();
    if (workspace) {
      return workspace.counts;
    }

    const stats = this.stats();
    return {
      assets: stats.discovered,
      files: stats.files,
      directories: stats.directories,
      archives: stats.archives,
      symlinks: stats.symlinks,
      totalBytes: stats.totalBytes,
    };
  });

  async pickFiles(): Promise<string[]> {
    const selection = await open({ multiple: true, directory: false });
    return normalizeSelection(selection);
  }

  async pickDirectories(): Promise<string[]> {
    const selection = await open({ multiple: true, directory: true });
    return normalizeSelection(selection);
  }

  async start(paths: string[]): Promise<boolean> {
    const sanitized = [...new Set(paths.filter((path) => path.trim().length > 0))];
    if (sanitized.length === 0 || this.busy()) {
      return false;
    }

    this.resetForScan();
    this.phase.set('scanning');

    try {
      const snapshot = await this.bridge.createWorkspace(sanitized, (event) => this.onEvent(event));
      this.workspace.set(snapshot);
      this.activeWorkspaceId.set(snapshot.id);
      this.stats.set({
        discovered: snapshot.counts.assets,
        files: snapshot.counts.files,
        directories: snapshot.counts.directories,
        archives: snapshot.counts.archives,
        symlinks: snapshot.counts.symlinks,
        totalBytes: snapshot.counts.totalBytes,
        warnings: this.warnings().length,
      });
      this.phase.set('ready');
      await this.loadInitialPage();
      return true;
    } catch (error) {
      this.error.set(errorMessage(error));
      this.phase.set('error');
      return false;
    }
  }

  async setFamilyFilter(family: FormatFamily | null): Promise<void> {
    if (this.familyFilter() === family) {
      return;
    }
    this.familyFilter.set(family);
    await this.loadInitialPage();
  }

  async loadMore(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || !this.hasMore()) {
      return;
    }

    const page = await this.bridge.listWorkspaceAssets(workspaceId, {
      offset: this.assets().length,
      limit: PAGE_SIZE,
      family: this.familyFilter(),
    });
    this.assets.update((current) => [...current, ...page.items]);
    this.pageTotal.set(page.total);
  }

  setDragActive(active: boolean): void {
    this.dragActive.set(active);
  }

  private onEvent(event: WorkspaceIntakeEvent): void {
    switch (event.event) {
      case 'started':
        this.activeWorkspaceId.set(event.data.workspaceId);
        break;
      case 'batch':
        this.stats.set(event.data.stats);
        this.assets.update((current) =>
          [...current, ...event.data.assets].slice(-PREVIEW_LIMIT),
        );
        break;
      case 'progress':
        this.stats.set(event.data.stats);
        break;
      case 'warning':
        this.stats.set(event.data.stats);
        this.warnings.update((current) => [...current, event.data.warning].slice(-50));
        break;
      case 'finished':
        this.workspace.set(event.data.workspace);
        break;
    }
  }

  private async loadInitialPage(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId) {
      return;
    }

    const page = await this.bridge.listWorkspaceAssets(workspaceId, {
      offset: 0,
      limit: PAGE_SIZE,
      family: this.familyFilter(),
    });
    this.assets.set(page.items);
    this.pageTotal.set(page.total);
  }

  private resetForScan(): void {
    this.workspace.set(null);
    this.activeWorkspaceId.set(null);
    this.stats.set({ ...EMPTY_STATS });
    this.assets.set([]);
    this.warnings.set([]);
    this.error.set(null);
    this.pageTotal.set(0);
    this.familyFilter.set(null);
  }
}

function normalizeSelection(selection: string | string[] | null): string[] {
  if (selection === null) {
    return [];
  }
  return Array.isArray(selection) ? selection : [selection];
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === 'string') {
    return error;
  }
  return 'Une erreur inattendue est survenue pendant l’analyse.';
}
