import { Injectable } from '@angular/core';
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import {
  ActionRecommendation,
  AssetPage,
  AssetQuery,
  CapabilityCatalog,
  ConversionPlan,
  DuplicateReport,
  EngineProbe,
  ExecuteWorkspaceActionRequest,
  ExecutionEvent,
  ExecutionSummary,
  HealthResponse,
  HistoryEntry,
  RecipeRecord,
  ScanOptions,
  SchedulerSnapshot,
  WorkspaceInsights,
  WorkspaceIntakeEvent,
  WorkspaceSnapshot,
} from './tauri.models';

@Injectable({ providedIn: 'root' })
export class TauriBridgeService {
  isDesktop(): boolean {
    return isTauri();
  }

  healthCheck(): Promise<HealthResponse> {
    return invoke<HealthResponse>('health_check');
  }

  probeEngines(): Promise<EngineProbe[]> {
    return invoke<EngineProbe[]>('probe_engines');
  }

  capabilityCatalog(): Promise<CapabilityCatalog> {
    return invoke<CapabilityCatalog>('capability_catalog');
  }

  planConversion(input: string, output: string): Promise<ConversionPlan> {
    return invoke<ConversionPlan>('plan_conversion', { input, output });
  }

  schedulerStatus(): Promise<SchedulerSnapshot> {
    return invoke<SchedulerSnapshot>('scheduler_status');
  }

  executableActions(): Promise<string[]> {
    return invoke<string[]>('executable_actions');
  }

  executeAction(
    request: ExecuteWorkspaceActionRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<ExecutionSummary> {
    const channel = new Channel<ExecutionEvent>();
    channel.onmessage = onEvent;
    return invoke<ExecutionSummary>('execute_action', { request, onEvent: channel });
  }

  cancelJob(jobId: string): Promise<boolean> {
    return invoke<boolean>('cancel_job', { jobId });
  }

  openJobOutput(jobId: string, index = 0): Promise<void> {
    return invoke<void>('open_job_output', { jobId, index });
  }

  revealJobOutput(jobId: string, index = 0): Promise<void> {
    return invoke<void>('reveal_job_output', { jobId, index });
  }

  saveJobOutputCopy(jobId: string, index = 0): Promise<string | null> {
    return invoke<string | null>('save_job_output_copy', { jobId, index });
  }

  history(limit = 100): Promise<HistoryEntry[]> {
    return invoke<HistoryEntry[]>('history', { limit });
  }

  favorites(): Promise<string[]> {
    return invoke<string[]>('favorites');
  }

  setFavorite(actionId: string, favorite: boolean): Promise<void> {
    return invoke<void>('set_favorite', { actionId, favorite });
  }

  recipes(): Promise<RecipeRecord[]> {
    return invoke<RecipeRecord[]>('recipes');
  }

  saveRecipe(recipe: RecipeRecord): Promise<void> {
    return invoke<void>('save_recipe', { recipe });
  }

  createWorkspace(
    paths: string[],
    onEvent: (event: WorkspaceIntakeEvent) => void,
    options?: Partial<ScanOptions>,
  ): Promise<WorkspaceSnapshot> {
    const channel = new Channel<WorkspaceIntakeEvent>();
    channel.onmessage = onEvent;

    return invoke<WorkspaceSnapshot>('create_workspace', {
      paths,
      options: options ?? null,
      onEvent: channel,
    });
  }

  getWorkspace(workspaceId: string): Promise<WorkspaceSnapshot> {
    return invoke<WorkspaceSnapshot>('get_workspace', { workspaceId });
  }

  listWorkspaceAssets(workspaceId: string, query: AssetQuery): Promise<AssetPage> {
    return invoke<AssetPage>('list_workspace_assets', { workspaceId, query });
  }

  workspaceInsights(workspaceId: string): Promise<WorkspaceInsights> {
    return invoke<WorkspaceInsights>('workspace_insights', { workspaceId });
  }

  workspaceRecommendations(workspaceId: string): Promise<ActionRecommendation[]> {
    return invoke<ActionRecommendation[]>('workspace_recommendations', { workspaceId });
  }

  confirmDuplicates(workspaceId: string): Promise<DuplicateReport> {
    return invoke<DuplicateReport>('confirm_duplicates', { workspaceId });
  }
}
