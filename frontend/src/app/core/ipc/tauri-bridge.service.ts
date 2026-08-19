import { Injectable } from '@angular/core';
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import {
  AssetPage,
  AssetQuery,
  EngineProbe,
  HealthResponse,
  ScanOptions,
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
}
