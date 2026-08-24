import { computed, inject, Injectable, signal } from '@angular/core';
import { TauriBridgeService } from '../../../core/ipc/tauri-bridge.service';
import { PreferencesService } from '../../../core/preferences/preferences.service';
import { UiMemoryService } from '../../../core/state/ui-memory.service';
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
  PreparedFilePreview,
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

const PREVIEW_LIMIT = 120;
const PAGE_SIZE = 200;
const TREE_PAGE_SIZE = 500;
const TREE_ASSET_LIMIT = 50_000;

@Injectable({ providedIn: 'root' })
export class WorkspaceStore {
  private readonly bridge = inject(TauriBridgeService);
  private readonly preferences = inject(PreferencesService);
  private readonly uiMemory = inject(UiMemoryService);
  private queryGeneration = 0;
  private intakeFlushScheduled = false;
  private executionFlushScheduled = false;
  private pendingIntakeStats: IntakeStats | null = null;
  private pendingIntakeAssets: Asset[] = [];
  private pendingWarnings: IntakeWarning[] = [];
  private pendingExecutionProgress: { completed: number; total: number } | null = null;
  private inspectedArchiveId: string | null = null;

  readonly phase = signal<WorkspacePhase>('idle');
  readonly workspace = signal<WorkspaceSnapshot | null>(null);
  readonly activeWorkspaceId = signal<string | null>(null);
  readonly stats = signal<IntakeStats>({ ...EMPTY_STATS });
  readonly assets = signal<Asset[]>([]);
  readonly treeAssets = signal<Asset[]>([]);
  readonly treeLoading = signal(false);
  readonly treeTruncated = signal(false);
  readonly warnings = signal<IntakeWarning[]>([]);
  readonly error = signal<string | null>(null);
  readonly dragActive = signal(false);
  readonly pageTotal = signal(0);
  readonly familyFilter = signal<FormatFamily | null>(null);
  readonly searchTerm = signal('');
  readonly sortBy = signal<AssetSortKey>('name');
  readonly sortDirection = signal<SortDirection>('ascending');
  readonly includeHidden = signal(this.preferences.showHidden());
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
  readonly executionPhase = signal('preparation');
  readonly executionPhaseActive = signal(false);
  readonly executionPhaseCompleted = signal(0);
  readonly executionPhaseTotal = signal(1);
  readonly executionBytesProcessed = signal(0);
  readonly executionBytesTotal = signal(0);
  readonly executionOutputBytes = signal(0);
  readonly executionBytesPerSecond = signal(0);
  readonly outputActionBusy = signal(false);
  readonly outputActionMessage = signal<string | null>(null);

  readonly busy = computed(() => this.phase() === 'scanning');
  readonly executing = computed(() => this.runningJobId() !== null);
  readonly executionProgress = computed(() => {
    if (this.executionBytesTotal() > 0 && this.executionPhase() === 'conversion') {
      const ratio = Math.min(1, this.executionBytesProcessed() / this.executionBytesTotal());
      return Math.round(12 + ratio * 74);
    }
    if (this.executionPhaseActive()) {
      const phase = this.executionPhase();
      const completed = this.executionPhaseCompleted();
      const total = Math.max(1, this.executionPhaseTotal());
      const ratio = Math.min(1, completed / total);
      if (phase === 'preparation') return Math.round(4 + ratio * 8);
      if (phase === 'conversion') return Math.round(12 + ratio * 58);
      if (phase === 'assemblage') return Math.round(72 + ratio * 12);
      if (phase === 'finalisation') return Math.round(86 + ratio * 8);
      if (phase === 'validation') return Math.round(94 + ratio * 5);
    }
    const batchTotal = this.executionTotal();
    return batchTotal > 0 ? Math.min(100, Math.round((this.executionCompleted() / batchTotal) * 100)) : 0;
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

  async pickFiles(extensions: string[] = []): Promise<string[]> {
    return this.bridge.pickFiles(extensions);
  }

  async pickDirectories(): Promise<string[]> {
    return this.bridge.pickDirectories();
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
      this.uiMemory.saveWorkspaceRoots(snapshot.roots);
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

  async restoreRememberedWorkspace(): Promise<boolean> {
    if (this.hasWorkspace() || this.busy()) return this.hasWorkspace();
    const roots = this.uiMemory.workspaceRoots();
    if (!roots.length) return false;
    return this.start(roots);
  }

  setPendingAction(actionId: string | null): void {
    this.pendingActionId.set(actionId);
  }

  startNewConversion(actionId: string | null): void {
    if (this.executing()) return;
    this.resetForScan();
    this.phase.set('idle');
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

  async loadTreeAssets(): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || this.treeLoading()) return;
    this.treeLoading.set(true);
    this.treeTruncated.set(false);
    try {
      const collected: Asset[] = [];
      let total = Number.POSITIVE_INFINITY;
      while (collected.length < total && collected.length < TREE_ASSET_LIMIT) {
        const page = await this.bridge.listWorkspaceAssets(workspaceId, {
          offset: collected.length,
          limit: TREE_PAGE_SIZE,
          includeHidden: true,
          sortBy: 'name',
          sortDirection: 'ascending',
        });
        total = page.total;
        collected.push(...page.items);
        if (!page.items.length) break;
      }
      if (this.workspace()?.id !== workspaceId) return;
      collected.sort((left, right) => left.data.relativePath.localeCompare(right.data.relativePath, 'fr', { numeric: true, sensitivity: 'base' }));
      this.treeAssets.set(collected);
      this.treeTruncated.set(collected.length < total);
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.treeLoading.set(false);
    }
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
    this.executionPhase.set('preparation');
    this.executionPhaseActive.set(false);
    this.executionPhaseCompleted.set(0);
    this.executionPhaseTotal.set(1);
    this.executionBytesProcessed.set(0);
    this.executionBytesTotal.set(0);
    this.executionOutputBytes.set(0);
    this.executionBytesPerSecond.set(0);
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

  resetExecutionResult(): void {
    if (this.executing()) return;
    this.executionSummary.set(null);
    this.executionError.set(null);
    this.executionFailures.set([]);
    this.outputActionMessage.set(null);
    this.executionCompleted.set(0);
    this.executionTotal.set(0);
    this.executionPhase.set('preparation');
    this.executionPhaseActive.set(false);
    this.executionPhaseCompleted.set(0);
    this.executionPhaseTotal.set(1);
    this.executionBytesProcessed.set(0);
    this.executionBytesTotal.set(0);
    this.executionOutputBytes.set(0);
    this.executionBytesPerSecond.set(0);
  }

  async refreshAfterMutation(): Promise<void> {
    if (!this.activeWorkspaceId()) return;
    this.clearSelection();
    await Promise.all([this.loadInitialPage(), this.loadWorkspaceDetails()]);
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

  async prepareAssetPreview(assetId: string): Promise<PreparedFilePreview> {
    const workspaceId = this.workspace()?.id ?? this.activeWorkspaceId();
    if (!workspaceId) throw new Error('Aucun espace de travail actif.');
    return this.bridge.previewAsset(workspaceId, assetId);
  }

  private onExecutionEvent(event: ExecutionEvent): void {
    switch (event.event) {
      case 'started':
        this.flushExecutionProgress();
        this.runningJobId.set(event.data.jobId);
        this.executionTotal.set(event.data.total);
        this.executionCompleted.set(0);
        break;
      case 'itemFailed':
        this.executionFailures.update((failures) => [...failures, `${event.data.input}: ${event.data.message}`].slice(-10));
        break;
      case 'progress':
        this.pendingExecutionProgress = { completed: event.data.completed, total: event.data.total };
        this.scheduleExecutionFlush();
        break;
      case 'phase':
        this.executionPhaseActive.set(true);
        this.executionPhase.set(event.data.phase);
        this.executionPhaseCompleted.set(event.data.completed);
        this.executionPhaseTotal.set(Math.max(1, event.data.total));
        break;
      case 'bytesProgress':
        this.executionBytesProcessed.set(event.data.processedBytes);
        this.executionBytesTotal.set(event.data.totalBytes);
        this.executionOutputBytes.set(event.data.outputBytes);
        this.executionBytesPerSecond.set(event.data.bytesPerSecond);
        break;
      case 'finished':
        this.flushExecutionProgress();
        this.executionSummary.set(event.data.summary);
        this.executionCompleted.set(event.data.summary.total);
        this.executionTotal.set(event.data.summary.total);
        break;
      case 'itemStarted':
      case 'itemCompleted':
        break;
    }
  }

  private scheduleExecutionFlush(): void {
    if (this.executionFlushScheduled) return;
    this.executionFlushScheduled = true;
    scheduleAnimationFrame(() => {
      this.executionFlushScheduled = false;
      this.flushExecutionProgress();
    });
  }

  private flushExecutionProgress(): void {
    const progress = this.pendingExecutionProgress;
    this.pendingExecutionProgress = null;
    if (!progress) return;
    this.executionCompleted.set(progress.completed);
    this.executionTotal.set(progress.total);
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

  async inspectArchive(offset = 0, limit = 24, archiveAssetId?: string | null): Promise<void> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId || this.archiveInspectionLoading()) return;
    const requestedArchive = archiveAssetId
      ? this.assets().find((asset) => asset.kind === 'archive' && asset.data.id === archiveAssetId)
      : null;
    const currentArchive = this.inspectedArchiveId
      ? this.assets().find((asset) => asset.kind === 'archive' && asset.data.id === this.inspectedArchiveId)
      : null;
    const selectedArchive = requestedArchive
      ?? currentArchive
      ?? this.selectedAssets().find((asset) => asset.kind === 'archive')
      ?? this.assets().find((asset) => asset.kind === 'archive');
    if (!selectedArchive || selectedArchive.kind !== 'archive') return;
    this.inspectedArchiveId = selectedArchive.data.id;
    this.archiveInspectionLoading.set(true);
    this.archiveInspectionError.set(null);
    try {
      this.archiveInspection.set(await this.bridge.inspectArchive(
        workspaceId,
        selectedArchive.data.id,
        Math.max(0, offset),
        Math.min(96, Math.max(1, limit)),
      ));
    } catch (error) {
      this.archiveInspectionError.set(errorMessage(error));
    } finally {
      this.archiveInspectionLoading.set(false);
    }
  }


  async previewArchiveEntry(entryPath: string): Promise<PreparedFilePreview | null> {
    const workspaceId = this.workspace()?.id;
    if (!workspaceId) return null;
    const selectedArchive = (this.inspectedArchiveId
      ? this.assets().find((asset) => asset.kind === 'archive' && asset.data.id === this.inspectedArchiveId)
      : null)
      ?? this.selectedAssets().find((asset) => asset.kind === 'archive')
      ?? this.assets().find((asset) => asset.kind === 'archive');
    if (!selectedArchive || selectedArchive.kind !== 'archive') return null;
    this.archiveInspectionError.set(null);
    try {
      return await this.bridge.previewArchiveEntry(workspaceId, entryPath, selectedArchive.data.id);
    } catch (error) {
      this.archiveInspectionError.set(errorMessage(error));
      return null;
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
        this.pendingIntakeStats = event.data.stats;
        this.pendingIntakeAssets.push(...event.data.assets);
        if (this.pendingIntakeAssets.length > PREVIEW_LIMIT * 2) {
          this.pendingIntakeAssets = this.pendingIntakeAssets.slice(-PREVIEW_LIMIT);
        }
        this.scheduleIntakeFlush();
        break;
      case 'progress':
        this.pendingIntakeStats = event.data.stats;
        this.scheduleIntakeFlush();
        break;
      case 'warning':
        this.pendingIntakeStats = event.data.stats;
        this.pendingWarnings.push(event.data.warning);
        this.scheduleIntakeFlush();
        break;
      case 'finished':
        this.flushIntakeEvents();
        this.workspace.set(event.data.workspace);
        break;
    }
  }

  private scheduleIntakeFlush(): void {
    if (this.intakeFlushScheduled) return;
    this.intakeFlushScheduled = true;
    scheduleAnimationFrame(() => {
      this.intakeFlushScheduled = false;
      this.flushIntakeEvents();
    });
  }

  private flushIntakeEvents(): void {
    if (this.pendingIntakeStats) {
      this.stats.set(this.pendingIntakeStats);
      this.pendingIntakeStats = null;
    }
    if (this.pendingIntakeAssets.length) {
      const pending = this.pendingIntakeAssets;
      this.pendingIntakeAssets = [];
      this.assets.update((current) => [...current, ...pending].slice(-PREVIEW_LIMIT));
    }
    if (this.pendingWarnings.length) {
      const pending = this.pendingWarnings;
      this.pendingWarnings = [];
      this.warnings.update((current) => [...current, ...pending].slice(-50));
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
    this.treeAssets.set([]);
    this.treeLoading.set(false);
    this.treeTruncated.set(false);
    this.warnings.set([]);
    this.archiveInspection.set(null);
    this.archiveInspectionError.set(null);
    this.inspectedArchiveId = null;
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
    this.executionPhase.set('preparation');
    this.executionPhaseActive.set(false);
    this.executionPhaseCompleted.set(0);
    this.executionPhaseTotal.set(1);
    this.executionBytesProcessed.set(0);
    this.executionBytesTotal.set(0);
    this.executionOutputBytes.set(0);
    this.executionBytesPerSecond.set(0);
    this.executionFailures.set([]);
    this.pendingIntakeStats = null;
    this.pendingIntakeAssets = [];
    this.pendingWarnings = [];
    this.pendingExecutionProgress = null;
  }
}

function scheduleAnimationFrame(callback: () => void): void {
  if (typeof globalThis.requestAnimationFrame === 'function') {
    globalThis.requestAnimationFrame(() => callback());
    return;
  }
  globalThis.setTimeout(callback, 16);
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Une erreur inattendue est survenue pendant l’analyse.';
}
