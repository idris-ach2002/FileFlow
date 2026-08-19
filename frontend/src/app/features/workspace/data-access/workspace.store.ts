import { computed, inject, Injectable, signal } from '@angular/core';
import { open } from '@tauri-apps/plugin-dialog';
import { TauriBridgeService } from '../../../core/ipc/tauri-bridge.service';
import {
  ActionRecommendation,
  Asset,
  AssetQuery,
  AssetSortKey,
  FormatFamily,
  IntakeStats,
  IntakeWarning,
  SortDirection,
  WorkspaceInsights,
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
  private queryGeneration = 0;

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
  readonly searchTerm = signal('');
  readonly sortBy = signal<AssetSortKey>('name');
  readonly sortDirection = signal<SortDirection>('ascending');
  readonly includeHidden = signal(true);
  readonly selectedIds = signal<ReadonlySet<string>>(new Set());
  readonly recommendations = signal<ActionRecommendation[]>([]);
  readonly insights = signal<WorkspaceInsights | null>(null);
  readonly detailsLoading = signal(false);
  readonly pendingActionId = signal<string | null>(null);
  readonly activeActionId = signal<string | null>(null);

  readonly busy = computed(() => this.phase() === 'scanning');
  readonly hasWorkspace = computed(() => this.workspace() !== null || this.activeWorkspaceId() !== null);
  readonly hasMore = computed(() => this.assets().length < this.pageTotal());
  readonly selectedCount = computed(() => this.selectedIds().size);
  readonly selectedAssets = computed(() => {
    const selected = this.selectedIds();
    return this.assets().filter((asset) => selected.has(asset.data.id));
  });
  readonly counts = computed(() => {
    const workspace = this.workspace();
    if (workspace) return workspace.counts;
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
    if (sanitized.length === 0 || this.busy()) return false;

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
      await Promise.all([this.loadInitialPage(), this.loadWorkspaceDetails()]);
      if (this.pendingActionId()) {
        this.activeActionId.set(this.pendingActionId());
        this.pendingActionId.set(null);
      }
      return true;
    } catch (error) {
      this.error.set(errorMessage(error));
      this.phase.set('error');
      return false;
    }
  }

  setPendingAction(actionId: string | null): void {
    this.pendingActionId.set(actionId);
  }

  openAction(actionId: string): void {
    this.activeActionId.set(actionId);
  }

  closeAction(): void {
    this.activeActionId.set(null);
  }

  async setFamilyFilter(family: FormatFamily | null): Promise<void> {
    if (this.familyFilter() === family) return;
    this.familyFilter.set(family);
    this.clearSelection();
    await this.loadInitialPage();
  }

  async setSearch(search: string): Promise<void> {
    const normalized = search.trim();
    if (this.searchTerm() === normalized) return;
    this.searchTerm.set(normalized);
    this.clearSelection();
    await this.loadInitialPage();
  }

  async setSort(sortBy: AssetSortKey): Promise<void> {
    if (this.sortBy() === sortBy) {
      this.sortDirection.update((direction) => direction === 'ascending' ? 'descending' : 'ascending');
    } else {
      this.sortBy.set(sortBy);
      this.sortDirection.set('ascending');
    }
    await this.loadInitialPage();
  }

  async setIncludeHidden(include: boolean): Promise<void> {
    if (this.includeHidden() === include) return;
    this.includeHidden.set(include);
    await this.loadInitialPage();
  }

  async loadMore(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || !this.hasMore()) return;
    const generation = this.queryGeneration;
    const page = await this.bridge.listWorkspaceAssets(workspaceId, this.query(this.assets().length));
    if (generation !== this.queryGeneration) return;
    this.assets.update((current) => [...current, ...page.items]);
    this.pageTotal.set(page.total);
  }

  toggleSelection(assetId: string): void {
    this.selectedIds.update((current) => {
      const next = new Set(current);
      if (next.has(assetId)) next.delete(assetId);
      else next.add(assetId);
      return next;
    });
  }

  selectVisible(): void {
    this.selectedIds.set(new Set(this.assets().map((asset) => asset.data.id)));
  }

  clearSelection(): void {
    this.selectedIds.set(new Set());
  }

  isSelected(assetId: string): boolean {
    return this.selectedIds().has(assetId);
  }

  setDragActive(active: boolean): void {
    this.dragActive.set(active);
  }

  async refreshDetails(): Promise<void> {
    await this.loadWorkspaceDetails();
  }

  private query(offset: number): AssetQuery {
    return {
      offset,
      limit: PAGE_SIZE,
      family: this.familyFilter(),
      search: this.searchTerm() || null,
      includeHidden: this.includeHidden(),
      sortBy: this.sortBy(),
      sortDirection: this.sortDirection(),
    };
  }

  private onEvent(event: WorkspaceIntakeEvent): void {
    switch (event.event) {
      case 'started':
        this.activeWorkspaceId.set(event.data.workspaceId);
        break;
      case 'batch':
        this.stats.set(event.data.stats);
        this.assets.update((current) => [...current, ...event.data.assets].slice(-PREVIEW_LIMIT));
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
    if (!workspaceId) return;
    const generation = ++this.queryGeneration;
    const page = await this.bridge.listWorkspaceAssets(workspaceId, this.query(0));
    if (generation !== this.queryGeneration) return;
    this.assets.set(page.items);
    this.pageTotal.set(page.total);
  }

  private async loadWorkspaceDetails(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId) return;
    this.detailsLoading.set(true);
    try {
      const [insights, recommendations] = await Promise.all([
        this.bridge.workspaceInsights(workspaceId),
        this.bridge.workspaceRecommendations(workspaceId),
      ]);
      this.insights.set(insights);
      this.recommendations.set(recommendations);
    } catch {
      // The asset list remains fully usable if optional intelligence fails.
    } finally {
      this.detailsLoading.set(false);
    }
  }

  private resetForScan(): void {
    this.queryGeneration += 1;
    this.workspace.set(null);
    this.activeWorkspaceId.set(null);
    this.stats.set({ ...EMPTY_STATS });
    this.assets.set([]);
    this.warnings.set([]);
    this.error.set(null);
    this.pageTotal.set(0);
    this.familyFilter.set(null);
    this.searchTerm.set('');
    this.selectedIds.set(new Set());
    this.recommendations.set([]);
    this.insights.set(null);
    this.activeActionId.set(null);
  }
}

function normalizeSelection(selection: string | string[] | null): string[] {
  if (selection === null) return [];
  return Array.isArray(selection) ? selection : [selection];
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Une erreur inattendue est survenue pendant l’analyse.';
}
