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

export interface FormatCapabilityProfile {
  id: string;
  label: string;
  family: FormatFamily;
  extensions: string[];
  preview: boolean;
  readable: boolean;
  writable: boolean;
  metadata: boolean;
  thumbnail: boolean;
  extractable: boolean;
  streamable: boolean;
  capabilities: string[];
  actions: string[];
  convertTo: string[];
  compressTo: string[];
}

export interface CapabilityCatalog {
  actions: ActionDescriptor[];
  conversions: ConversionEdge[];
  formats: FormatCapabilityProfile[];
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


export type JobState =
  | 'queued'
  | 'preparing'
  | 'waitingForResources'
  | 'running'
  | 'finalizing'
  | 'completed'
  | 'failed'
  | 'cancelling'
  | 'cancelled';

export type DestinationPolicy = 'sameFolder' | 'subfolder' | 'customFolder' | 'askEveryTime';
export type ConflictStrategy = 'increment' | 'skip' | 'replace' | 'ask';
export type NamingStrategy = 'original' | 'operationSuffix' | 'dateSuffix';

export interface OutputPolicy {
  destination: DestinationPolicy;
  customDirectory?: string | null;
  subfolderName: string;
  preserveTree: boolean;
  conflict: ConflictStrategy;
  naming: NamingStrategy;
  overwriteOriginal: boolean;
}

export interface ExecuteWorkspaceActionRequest {
  workspaceId: string;
  actionId: string;
  selectedAssetIds: string[];
  outputPolicy: OutputPolicy;
  targetFormat?: string | null;
  quality?: 'small' | 'balanced' | 'high' | null;
  parameters?: Record<string, string | number | boolean | null>;
}

export interface ItemFailure {
  input: string;
  message: string;
}

export interface ExecutionSummary {
  jobId: string;
  actionId: string;
  state: JobState;
  total: number;
  succeeded: number;
  skipped: number;
  failed: number;
  outputs: string[];
  failures: ItemFailure[];
  durationMs: number;
  finishedAt: string;
}

export type ExecutionEvent =
  | { event: 'started'; data: { jobId: string; actionId: string; total: number } }
  | { event: 'itemStarted'; data: { jobId: string; index: number; input: string } }
  | { event: 'itemCompleted'; data: { jobId: string; index: number; input: string; output?: string | null; skipped: boolean } }
  | { event: 'itemFailed'; data: { jobId: string; index: number; input: string; message: string } }
  | { event: 'progress'; data: { jobId: string; completed: number; total: number } }
  | { event: 'finished'; data: { summary: ExecutionSummary } };

export interface HistoryEntry {
  id: string;
  actionId: string;
  inputCount: number;
  outputCount: number;
  inputBytes: number;
  outputBytes: number;
  destination?: string | null;
  status: string;
  durationMs: number;
  createdAt: string;
}

export interface RecipeRecord {
  id: string;
  name: string;
  description: string;
  icon: string;
  stepsJson: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}


export interface ConfirmedDuplicateAsset {
  assetId: string;
  path: string;
  sizeBytes: number;
}

export interface ConfirmedDuplicateGroup {
  hash: string;
  sizeBytes: number;
  reclaimableBytes: number;
  assets: ConfirmedDuplicateAsset[];
}

export interface AnalysisWarning {
  path: string;
  message: string;
}

export interface DuplicateReport {
  inputFiles: number;
  sizeCandidateFiles: number;
  quickCandidateFiles: number;
  fullyHashedFiles: number;
  confirmedGroups: ConfirmedDuplicateGroup[];
  reclaimableBytes: number;
  warnings: AnalysisWarning[];
}

export interface ArchiveFamilySummary {
  family: FormatFamily;
  count: number;
  totalBytes: number;
}

export interface ArchiveEntryPreview {
  path: string;
  sizeBytes: number;
  family: FormatFamily;
}

export interface ArchiveInspection {
  entries: number;
  files: number;
  directories: number;
  totalUnpackedBytes: number;
  families: ArchiveFamilySummary[];
  samples: ArchiveEntryPreview[];
}

export interface AccountBootstrap {
  hasAccount: boolean;
}

export interface AccountProfile {
  id: string;
  email: string;
  displayName: string;
  firstName: string;
  lastName: string;
  avatarPath?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface OnboardingPreferences {
  accountId: string;
  completed: boolean;
  storageDirectory?: string | null;
  language: 'fr' | 'en' | 'de' | string;
  beginnerMode: boolean;
  preserveOriginals: boolean;
  notifications: boolean;
  confirmDestructiveActions: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AuthSessionResponse {
  token: string;
  expiresAt: string;
  profile: AccountProfile;
  onboarding: OnboardingPreferences;
}

export interface CreateAccountRequest {
  email: string;
  password: string;
  displayName: string;
  firstName: string;
  lastName: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface ChangePasswordRequest {
  currentPassword: string;
  newPassword: string;
}

export interface ProfileUpdate {
  displayName: string;
  firstName: string;
  lastName: string;
  email: string;
}

export interface AvatarPayload {
  mimeType: string;
  bytes: number[];
}

export interface WorkflowStep {
  id: string;
  actionId: string;
  dependsOn: string[];
  targetFormat?: string | null;
  quality?: string | null;
  parameters: Record<string, string | number | boolean | null>;
  outputPolicy: OutputPolicy;
}

export interface WorkflowDefinition {
  version: number;
  name: string;
  description: string;
  steps: WorkflowStep[];
}

export interface WorkflowEvent {
  event: 'started' | 'stepStarted' | 'stepCompleted' | 'finished' | string;
  jobId: string;
  stepId?: string | null;
  completedSteps: number;
  totalSteps: number;
  message?: string | null;
}

export interface AutomationJobRecord {
  id: string;
  recipeId?: string | null;
  status: string;
  currentStep: number;
  totalSteps: number;
  inputPaths: string[];
  outputsByStep: Record<string, string[]>;
  error?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface WatchedFolderRecord {
  id: string;
  path: string;
  recipeId: string;
  enabled: boolean;
  recursive: boolean;
  extensions: string[];
  stabilitySeconds: number;
  lastScanAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface SaveWatchedFolderRequest {
  id?: string | null;
  path: string;
  recipeId: string;
  enabled: boolean;
  recursive: boolean;
  extensions: string[];
  stabilitySeconds: number;
}

export interface RenameRule {
  template: string;
  search: string;
  replace: string;
  counterStart: number;
  counterPadding: number;
  caseMode: 'keep' | 'lower' | 'upper' | 'title' | string;
  preserveExtension: boolean;
}

export interface RenamePreviewItem {
  assetId: string;
  source: string;
  target: string;
  changed: boolean;
  conflict: boolean;
  warning?: string | null;
}

export interface RenamePreview {
  items: RenamePreviewItem[];
  total: number;
  changed: number;
  conflicts: number;
  truncated: boolean;
}

export interface OrganizationPreviewItem {
  assetId: string;
  source: string;
  target: string;
  category: string;
  conflictResolved: boolean;
}

export interface OrganizationPreview {
  items: OrganizationPreviewItem[];
  total: number;
  truncated: boolean;
  categories: Record<string, number>;
}

export interface DuplicateCleanupGroup {
  hash: string;
  sizeBytes: number;
  keepAssetId: string;
  keepPath: string;
  quarantineAssetIds: string[];
  quarantinePaths: string[];
  reclaimableBytes: number;
}

export interface DuplicateCleanupPlan {
  groups: DuplicateCleanupGroup[];
  reclaimableBytes: number;
  quarantineCount: number;
  warnings: string[];
}

export interface FileOperationResult {
  processed: number;
  destination?: string | null;
  warnings: string[];
}
