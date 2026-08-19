import { Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { EngineProbe, HealthResponse } from './tauri.models';

@Injectable({ providedIn: 'root' })
export class TauriBridgeService {
  healthCheck(): Promise<HealthResponse> {
    return invoke<HealthResponse>('health_check');
  }

  probeEngines(): Promise<EngineProbe[]> {
    return invoke<EngineProbe[]>('probe_engines');
  }
}
