import { computed, inject, Injectable, signal } from '@angular/core';
import { TauriBridgeService } from '../../core/ipc/tauri-bridge.service';
import {
  AutomationJobRecord,
  RecipeRecord,
  SaveWatchedFolderRequest,
  WatchedFolderRecord,
  WorkflowEvent,
} from '../../core/ipc/tauri.models';

@Injectable({ providedIn: 'root' })
export class AutomationStore {
  private readonly bridge = inject(TauriBridgeService);
  readonly recipes = signal<RecipeRecord[]>([]);
  readonly jobs = signal<AutomationJobRecord[]>([]);
  readonly watchedFolders = signal<WatchedFolderRecord[]>([]);
  readonly loading = signal(false);
  readonly runningJobId = signal<string | null>(null);
  readonly workflowEvent = signal<WorkflowEvent | null>(null);
  readonly error = signal<string | null>(null);
  readonly notice = signal<string | null>(null);

  readonly recoverableJobs = computed(() =>
    this.jobs().filter((job) => ['interrupted', 'failed', 'cancelled'].includes(job.status)),
  );
  readonly activeJobs = computed(() =>
    this.jobs().filter((job) => ['queued', 'running', 'waitingForResources', 'finalizing'].includes(job.status)),
  );

  load(): void {
    if (!this.bridge.isDesktop() || this.loading()) return;
    this.loading.set(true);
    this.error.set(null);
    void Promise.all([
      this.bridge.recipes(),
      this.bridge.automationJobs(100),
      this.bridge.watchedFolders(),
    ]).then(
      ([recipes, jobs, watchedFolders]) => {
        this.recipes.set(recipes);
        this.jobs.set(jobs);
        this.watchedFolders.set(watchedFolders);
        this.loading.set(false);
      },
      (error: unknown) => {
        this.error.set(message(error));
        this.loading.set(false);
      },
    );
  }

  async save(recipe: RecipeRecord): Promise<boolean> {
    this.error.set(null);
    try {
      await this.bridge.saveRecipe(recipe);
      this.load();
      return true;
    } catch (error) {
      this.error.set(message(error));
      return false;
    }
  }

  async run(recipeId: string, inputPaths: string[]): Promise<AutomationJobRecord | null> {
    if (this.runningJobId() || inputPaths.length === 0) return null;
    this.error.set(null);
    this.notice.set(null);
    this.workflowEvent.set(null);
    try {
      const job = await this.bridge.runRecipe(recipeId, inputPaths, (event) => {
        this.workflowEvent.set(event);
        this.runningJobId.set(event.event === 'finished' ? null : event.jobId);
      });
      this.runningJobId.set(null);
      this.notice.set(job.status === 'completed' ? 'Workflow terminé.' : null);
      this.load();
      return job;
    } catch (error) {
      this.runningJobId.set(null);
      this.error.set(message(error));
      this.load();
      return null;
    }
  }

  async runWorkspace(recipeId: string, workspaceId: string, selectedAssetIds: string[]): Promise<AutomationJobRecord | null> {
    if (this.runningJobId()) return null;
    this.error.set(null);
    this.notice.set(null);
    this.workflowEvent.set(null);
    try {
      const job = await this.bridge.runRecipeOnWorkspace(recipeId, workspaceId, selectedAssetIds, (event) => {
        this.workflowEvent.set(event);
        this.runningJobId.set(event.event === 'finished' ? null : event.jobId);
      });
      this.runningJobId.set(null);
      this.notice.set(job.status === 'completed' ? 'Workflow terminé.' : null);
      this.load();
      return job;
    } catch (error) {
      this.runningJobId.set(null);
      this.error.set(message(error));
      this.load();
      return null;
    }
  }

  async resume(jobId: string): Promise<AutomationJobRecord | null> {
    if (this.runningJobId()) return null;
    this.error.set(null);
    try {
      this.runningJobId.set(jobId);
      const job = await this.bridge.resumeAutomationJob(jobId, (event) => {
        this.workflowEvent.set(event);
        this.runningJobId.set(event.event === 'finished' ? null : event.jobId);
      });
      this.runningJobId.set(null);
      this.load();
      return job;
    } catch (error) {
      this.runningJobId.set(null);
      this.error.set(message(error));
      return null;
    }
  }

  async cancel(jobId?: string): Promise<void> {
    const id = jobId ?? this.runningJobId();
    if (!id) return;
    await this.bridge.cancelAutomationJob(id);
  }

  async saveWatch(request: SaveWatchedFolderRequest): Promise<boolean> {
    this.error.set(null);
    try {
      await this.bridge.saveWatchedFolder(request);
      this.notice.set('Dossier surveillé enregistré.');
      this.load();
      return true;
    } catch (error) {
      this.error.set(message(error));
      return false;
    }
  }

  async deleteWatch(watchId: string): Promise<void> {
    try {
      await this.bridge.deleteWatchedFolder(watchId);
      this.load();
    } catch (error) {
      this.error.set(message(error));
    }
  }
}

function message(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'L’automatisation n’a pas pu être exécutée.';
}
