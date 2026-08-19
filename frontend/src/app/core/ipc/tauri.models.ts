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
}

export interface AssetPage {
  workspaceId: string;
  offset: number;
  limit: number;
  total: number;
  items: Asset[];
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
