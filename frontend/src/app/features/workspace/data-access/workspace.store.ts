import { computed, inject, Injectable, signal } from '@angular/core';
import { open } from '@tauri-apps/plugin-dialog';
import { TauriBridgeService } from '../../../core/ipc/tauri-bridge.service';
import {
  ActionRecommendation,
  ArchiveInspection,
  Asset,
  DuplicateReport,
  ExecuteWorkspaceActionRequest,
  ExecutionEvent,
  ExecutionSummary,
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
  readonly duplicateReport = signal<DuplicateReport | null>(null);
  readonly duplicateScanLoading = signal(false);
  readonly duplicateScanError = signal<string | null>(null);
  readonly archiveInspection = signal<ArchiveInspection | null>(null);
  readonly archiveInspectionLoading = signal(false);
  readonly archiveInspectionError = signal<string | null>(null);
  readonly pendingActionId = signal<string | null>(null);
  readonly activeActionId = signal<string | null>(null);
  readonly executionSummary = signal<ExecutionSummary | null>(null);
  readonly executionError = signal<string | null>(null);
  readonly runningJobId = signal<string | null>(null);
  readonly executionCompleted = signal(0);
  readonly executionTotal = signal(0);
  readonly executionFailures = signal<string[]>([]);
  readonly outputActionBusy = signal(false);
  readonly outputActionMessage = signal<string | null>(null);

  readonly busy = computed(() => this.phase() === 'scanning');
  readonly executing = computed(() => this.runningJobId() !== null);
  readonly executionProgress = computed(() => {
    const total = this.executionTotal();
    return total > 0 ? Math.min(100, Math.round((this.executionCompleted() / total) * 100)) : 0;
  });
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
    if (!this.executing()) this.activeActionId.set(null);
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

  async executeAction(request: ExecuteWorkspaceActionRequest): Promise<ExecutionSummary | null> {
    if (this.executing()) return null;
    this.executionSummary.set(null);
    this.executionError.set(null);
    this.executionFailures.set([]);
    this.outputActionMessage.set(null);
    this.executionCompleted.set(0);
    this.executionTotal.set(0);
    try {
      const summary = await this.bridge.executeAction(request, (event) => this.onExecutionEvent(event));
      this.executionSummary.set(summary);
      if (summary.state !== 'cancelled') {
        await Promise.all([this.loadInitialPage(), this.loadWorkspaceDetails()]);
      }
      return summary;
    } catch (error) {
      this.executionError.set(errorMessage(error));
      return null;
    } finally {
      this.runningJobId.set(null);
    }
  }

  async cancelExecution(): Promise<void> {
    const jobId = this.runningJobId();
    if (!jobId) return;
    await this.bridge.cancelJob(jobId);
  }

  async openOutput(index = 0): Promise<void> {
    const summary = this.executionSummary();
    if (!summary?.outputs[index] || this.outputActionBusy()) return;
    this.outputActionBusy.set(true);
    this.outputActionMessage.set(null);
    try {
      await this.bridge.openJobOutput(summary.jobId, index);
    } catch (error) {
      this.outputActionMessage.set(errorMessage(error));
    } finally {
      this.outputActionBusy.set(false);
    }
  }

  async revealOutput(index = 0): Promise<void> {
    const summary = this.executionSummary();
    if (!summary?.outputs[index] || this.outputActionBusy()) return;
    this.outputActionBusy.set(true);
    this.outputActionMessage.set(null);
    try {
      await this.bridge.revealJobOutput(summary.jobId, index);
    } catch (error) {
      this.outputActionMessage.set(errorMessage(error));
    } finally {
      this.outputActionBusy.set(false);
    }
  }

  async saveOutputCopy(index = 0): Promise<void> {
    const summary = this.executionSummary();
    if (!summary?.outputs[index] || this.outputActionBusy()) return;
    this.outputActionBusy.set(true);
    this.outputActionMessage.set(null);
    try {
      const copied = await this.bridge.saveJobOutputCopy(summary.jobId, index);
      if (copied) this.outputActionMessage.set(`Copie enregistrée : ${copied}`);
    } catch (error) {
      this.outputActionMessage.set(errorMessage(error));
    } finally {
      this.outputActionBusy.set(false);
    }
  }

  private onExecutionEvent(event: ExecutionEvent): void {
    switch (event.event) {
      case 'started':
        this.runningJobId.set(event.data.jobId);
        this.executionTotal.set(event.data.total);
        this.executionCompleted.set(0);
        break;
      case 'itemFailed':
        this.executionFailures.update((failures) => [...failures, `${event.data.input}: ${event.data.message}`].slice(-10));
        break;
      case 'progress':
        this.executionCompleted.set(event.data.completed);
        this.executionTotal.set(event.data.total);
        break;
      case 'finished':
        this.executionSummary.set(event.data.summary);
        this.executionCompleted.set(event.data.summary.total);
        this.executionTotal.set(event.data.summary.total);
        break;
      case 'itemStarted':
      case 'itemCompleted':
        break;
    }
  }

  setDragActive(active: boolean): void {
    this.dragActive.set(active);
  }

  async refreshDetails(): Promise<void> {
    await this.loadWorkspaceDetails();
  }

  async confirmDuplicates(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || this.duplicateScanLoading()) return;
    this.duplicateScanLoading.set(true);
    this.duplicateScanError.set(null);
    try {
      this.duplicateReport.set(await this.bridge.confirmDuplicates(workspaceId));
    } catch (error) {
      this.duplicateScanError.set(errorMessage(error));
    } finally {
      this.duplicateScanLoading.set(false);
    }
  }

  async inspectArchive(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || this.archiveInspectionLoading()) return;
    const selectedArchive = this.selectedAssets().find((asset) => asset.kind === 'archive');
    this.archiveInspectionLoading.set(true);
    this.archiveInspectionError.set(null);
    try {
      this.archiveInspection.set(await this.bridge.inspectArchive(workspaceId, selectedArchive?.data.id ?? null));
    } catch (error) {
      this.archiveInspectionError.set(errorMessage(error));
    } finally {
      this.archiveInspectionLoading.set(false);
    }
  }

  async analyzeOutput(index = 0): Promise<boolean> {
    const output = this.executionSummary()?.outputs[index];
    if (!output) return false;
    return this.start([output]);
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
    this.archiveInspection.set(null);
    this.archiveInspectionError.set(null);
    this.error.set(null);
    this.pageTotal.set(0);
    this.familyFilter.set(null);
    this.searchTerm.set('');
    this.selectedIds.set(new Set());
    this.recommendations.set([]);
    this.insights.set(null);
    this.duplicateReport.set(null);
    this.duplicateScanLoading.set(false);
    this.duplicateScanError.set(null);
    this.activeActionId.set(null);
    this.executionSummary.set(null);
    this.executionError.set(null);
    this.runningJobId.set(null);
    this.executionCompleted.set(0);
    this.executionTotal.set(0);
    this.executionFailures.set([]);
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
