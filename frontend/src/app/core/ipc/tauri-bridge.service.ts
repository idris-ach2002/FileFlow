import { Injectable } from '@angular/core';
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import {
  ActionRecommendation,
  AssetPage,
  AssetQuery,
  CapabilityCatalog,
  ConversionPlan,
  EngineProbe,
  HealthResponse,
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
}
