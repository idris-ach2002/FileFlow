export interface HealthResponse {
  app: string;
  version: string;
  cpuThreads: number;
  os: string;
  architecture: string;
}

export interface EngineProbe {
  id: string;
  displayName: string;
  available: boolean;
  executable?: string | null;
}
