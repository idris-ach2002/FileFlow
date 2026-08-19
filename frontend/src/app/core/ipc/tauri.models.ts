export interface ResourceBudget {
  cpuTokens: number;
  memoryMb: number;
  ioTokens: number;
}

export interface SchedulerSnapshot {
  budget: ResourceBudget;
  cpuAvailable: number;
  memoryMbAvailable: number;
  ioAvailable: number;
}

export interface ResourceProfile {
  cpuWeight: number;
  memoryMb: number;
  ioWeight: number;
  internallyThreaded: boolean;
  maxParallelInstances: number;
}

export interface HealthResponse {
  app: string;
  version: string;
  cpuThreads: number;
  os: string;
  architecture: string;
  scheduler: SchedulerSnapshot;
}

export interface EngineProbe {
  id: string;
  displayName: string;
  available: boolean;
  executable?: string | null;
  resourceProfile: ResourceProfile;
}

export type FormatFamily =
  | 'image'
  | 'pdf'
  | 'document'
  | 'spreadsheet'
  | 'presentation'
  | 'audio'
  | 'video'
  | 'archive'
  | 'ebook'
  | 'text'
  | 'unknown';

export type AssetKind = 'file' | 'directory' | 'archive' | 'symlink' | 'other';
export type DetectionConfidence = 'unknown' | 'extension' | 'magic';
export type WorkspaceStatus = 'scanning' | 'ready' | 'failed';
export type AssetSortKey = 'name' | 'size' | 'modified' | 'format' | 'family';
export type SortDirection = 'ascending' | 'descending';
export type OperationCategory =
  | 'convert'
  | 'pdf'
  | 'image'
  | 'document'
  | 'media'
  | 'archive'
  | 'extract'
  | 'organize'
  | 'privacy'
  | 'optimize';
export type ActionScope = 'single' | 'batch' | 'workspace';

export interface DetectedFormat {
  id: string;
  extension?: string | null;
  mimeType?: string | null;
  family: FormatFamily;
  confidence: DetectionConfidence;
}

export interface AssetCommon {
  id: string;
  rootIndex: number;
  path: string;
  relativePath: string;
  name: string;
  hidden: boolean;
  modifiedAt?: string | null;
}

export interface FileAsset extends AssetCommon {
  sizeBytes: number;
  format: DetectedFormat;
}

export interface DirectoryAsset extends AssetCommon {}

export interface ArchiveAsset extends AssetCommon {
  sizeBytes: number;
  format: DetectedFormat;
}

export interface SymlinkAsset extends AssetCommon {
  target?: string | null;
}

export type Asset =
  | { kind: 'file'; data: FileAsset }
  | { kind: 'directory'; data: DirectoryAsset }
  | { kind: 'archive'; data: ArchiveAsset }
  | { kind: 'symlink'; data: SymlinkAsset };

export interface IntakeStats {
  discovered: number;
  files: number;
  directories: number;
  archives: number;
  symlinks: number;
  totalBytes: number;
  warnings: number;
}

export interface IntakeWarning {
  path: string;
  code: string;
  message: string;
}

export interface ScanOptions {
  recursive: boolean;
  followSymlinks: boolean;
  includeHidden: boolean;
  maxDepth?: number | null;
  batchSize: number;
  sampleBytes: number;
}

export interface WorkspaceCounts {
  assets: number;
  files: number;
  directories: number;
  archives: number;
  symlinks: number;
  totalBytes: number;
}

export interface FamilyCount {
  family: FormatFamily;
  count: number;
}

export interface WorkspaceSnapshot {
  id: string;
  status: WorkspaceStatus;
  roots: string[];
  counts: WorkspaceCounts;
  families: FamilyCount[];
  createdAt: string;
  updatedAt: string;
  error?: string | null;
}

export interface AssetQuery {
  offset?: number;
  limit?: number;
  family?: FormatFamily | null;
  kind?: AssetKind | null;
  search?: string | null;
  includeHidden?: boolean;
  sortBy?: AssetSortKey;
  sortDirection?: SortDirection;
}

export interface AssetPage {
  workspaceId: string;
  offset: number;
  limit: number;
  total: number;
  items: Asset[];
}

export interface ActionDescriptor {
  id: string;
  title: string;
  description: string;
  category: OperationCategory;
  scopes: ActionScope[];
  accepts: FormatFamily[];
  outputFormat?: string | null;
  requiredEngines: string[];
  batchable: boolean;
  destructive: boolean;
  featured: boolean;
}

export interface ActionRecommendation {
  actionId: string;
  score: number;
  reason: string;
  affectedAssets: number;
  ready: boolean;
  missingEngines: string[];
}

export interface ConversionEdge {
  from: string;
  to: string;
  engineId: string;
  cost: number;
  lossy: boolean;
}

export interface ConversionStep extends ConversionEdge {}

export interface ConversionPlan {
  input: string;
  output: string;
  totalCost: number;
  steps: ConversionStep[];
}

export interface CapabilityCatalog {
  actions: ActionDescriptor[];
  conversions: ConversionEdge[];
}

export interface ExtensionCount {
  extension: string;
  count: number;
  totalBytes: number;
}

export interface AssetInsight {
  id: string;
  name: string;
  relativePath: string;
  family: FormatFamily;
  sizeBytes: number;
}

export interface DuplicateSizeCandidate {
  sizeBytes: number;
  count: number;
  reclaimableUpperBound: number;
  samples: AssetInsight[];
}

export interface WorkspaceInsights {
  hiddenAssets: number;
  unknownAssets: number;
  extensionCount: number;
  extensions: ExtensionCount[];
  largest: AssetInsight[];
  duplicateSizeCandidates: DuplicateSizeCandidate[];
  potentialDuplicateBytes: number;
}

export type WorkspaceIntakeEvent =
  | {
      event: 'started';
      data: { workspaceId: string; roots: number };
    }
  | {
      event: 'batch';
      data: { workspaceId: string; assets: Asset[]; stats: IntakeStats };
    }
  | {
      event: 'progress';
      data: { workspaceId: string; stats: IntakeStats };
    }
  | {
      event: 'warning';
      data: { workspaceId: string; warning: IntakeWarning; stats: IntakeStats };
    }
  | {
      event: 'finished';
      data: { workspace: WorkspaceSnapshot };
    };
