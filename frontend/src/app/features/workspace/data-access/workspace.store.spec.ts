import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { TauriBridgeService } from '../../../core/ipc/tauri-bridge.service';
import {
  Asset,
  IntakeStats,
  WorkspaceIntakeEvent,
  WorkspaceSnapshot,
} from '../../../core/ipc/tauri.models';
import { WorkspaceStore } from './workspace.store';

const stats: IntakeStats = {
  discovered: 1,
  files: 1,
  directories: 0,
  archives: 0,
  symlinks: 0,
  totalBytes: 5,
  warnings: 0,
};

const asset: Asset = {
  kind: 'file',
  data: {
    id: 'asset-1',
    rootIndex: 0,
    path: '/tmp/hello.txt',
    relativePath: 'hello.txt',
    name: 'hello.txt',
    hidden: false,
    modifiedAt: null,
    sizeBytes: 5,
    format: {
      id: 'text',
      extension: 'txt',
      mimeType: 'text/plain',
      family: 'text',
      confidence: 'extension',
    },
  },
};

const snapshot: WorkspaceSnapshot = {
  id: 'workspace-1',
  status: 'ready',
  roots: ['/tmp/hello.txt'],
  counts: {
    assets: 1,
    files: 1,
    directories: 0,
    archives: 0,
    symlinks: 0,
    totalBytes: 5,
  },
  families: [{ family: 'text', count: 1 }],
  createdAt: '2026-08-19T20:00:00Z',
  updatedAt: '2026-08-19T20:00:01Z',
  error: null,
};

describe('WorkspaceStore', () => {
  it('streams intake state then loads the paged workspace result', async () => {
    const bridge = {
      isDesktop: vi.fn().mockReturnValue(false),
      createWorkspace: vi.fn(
        async (_paths: string[], onEvent: (event: WorkspaceIntakeEvent) => void) => {
          onEvent({ event: 'started', data: { workspaceId: snapshot.id, roots: 1 } });
          onEvent({
            event: 'batch',
            data: { workspaceId: snapshot.id, assets: [asset], stats },
          });
          onEvent({ event: 'finished', data: { workspace: snapshot } });
          return snapshot;
        },
      ),
      listWorkspaceAssets: vi.fn().mockResolvedValue({
        workspaceId: snapshot.id,
        offset: 0,
        limit: 200,
        total: 1,
        items: [asset],
      }),
      workspaceInsights: vi.fn().mockResolvedValue({
        hiddenAssets: 0, unknownAssets: 0, extensionCount: 1,
        extensions: [{ extension: 'txt', count: 1, totalBytes: 5 }],
        largest: [], duplicateSizeCandidates: [], potentialDuplicateBytes: 0,
      }),
      workspaceRecommendations: vi.fn().mockResolvedValue([]),
    };

    TestBed.configureTestingModule({
      providers: [
        WorkspaceStore,
        { provide: TauriBridgeService, useValue: bridge },
      ],
    });

    const store = TestBed.inject(WorkspaceStore);
    const completed = await store.start(['/tmp/hello.txt']);

    expect(completed).toBe(true);
    expect(store.phase()).toBe('ready');
    expect(store.workspace()?.id).toBe(snapshot.id);
    expect(store.counts().files).toBe(1);
    expect(store.assets()).toEqual([asset]);
    expect(bridge.listWorkspaceAssets).toHaveBeenCalledOnce();
  });

  it('keeps the inspected ZIP identity and returns its prepared entry preview', async () => {
    const archive: Asset = {
      kind: 'archive',
      data: {
        id: 'archive-2', rootIndex: 0, path: '/tmp/documents.zip',
        relativePath: 'documents.zip', name: 'documents.zip', hidden: false,
        modifiedAt: null, sizeBytes: 1024,
        format: { id: 'zip', extension: 'zip', mimeType: 'application/zip', family: 'archive', confidence: 'extension' },
      },
    };
    const archiveSnapshot: WorkspaceSnapshot = {
      ...snapshot,
      roots: ['/tmp/documents.zip'],
      counts: { assets: 1, files: 0, directories: 0, archives: 1, symlinks: 0, totalBytes: 1024 },
      families: [{ family: 'archive', count: 1 }],
    };
    const archiveStats: IntakeStats = {
      discovered: 1, files: 0, directories: 0, archives: 1,
      symlinks: 0, totalBytes: 1024, warnings: 0,
    };
    const prepared = { path: '/tmp/fileflow-previews/first-page.png', family: 'image' as const, generated: true };
    const bridge = {
      isDesktop: vi.fn().mockReturnValue(false),
      createWorkspace: vi.fn(async (_paths: string[], onEvent: (event: WorkspaceIntakeEvent) => void) => {
        onEvent({ event: 'started', data: { workspaceId: archiveSnapshot.id, roots: 1 } });
        onEvent({ event: 'batch', data: { workspaceId: archiveSnapshot.id, assets: [archive], stats: archiveStats } });
        onEvent({ event: 'finished', data: { workspace: archiveSnapshot } });
        return archiveSnapshot;
      }),
      listWorkspaceAssets: vi.fn().mockResolvedValue({
        workspaceId: archiveSnapshot.id, offset: 0, limit: 200, total: 1, items: [archive],
      }),
      workspaceInsights: vi.fn().mockResolvedValue({
        hiddenAssets: 0, unknownAssets: 0, extensionCount: 1,
        extensions: [{ extension: 'zip', count: 1, totalBytes: 1024 }],
        largest: [], duplicateSizeCandidates: [], potentialDuplicateBytes: 0,
      }),
      workspaceRecommendations: vi.fn().mockResolvedValue([]),
      inspectArchive: vi.fn().mockResolvedValue({
        entries: 1, files: 1, directories: 0, totalUnpackedBytes: 500,
        families: [{ family: 'pdf', count: 1, totalBytes: 500 }],
        samples: [{ path: 'docs/report.pdf', sizeBytes: 500, family: 'pdf' }],
        offset: 0, limit: 24, hasMore: false,
      }),
      previewArchiveEntry: vi.fn().mockResolvedValue(prepared),
    };

    TestBed.configureTestingModule({
      providers: [
        WorkspaceStore,
        { provide: TauriBridgeService, useValue: bridge },
      ],
    });

    const store = TestBed.inject(WorkspaceStore);
    expect(await store.start(['/tmp/documents.zip'])).toBe(true);
    await store.inspectArchive(0, 24, 'archive-2');
    expect(await store.previewArchiveEntry('docs/report.pdf')).toEqual(prepared);
    expect(bridge.inspectArchive).toHaveBeenCalledWith(archiveSnapshot.id, 'archive-2', 0, 24);
    expect(bridge.previewArchiveEntry).toHaveBeenCalledWith(
      archiveSnapshot.id,
      'docs/report.pdf',
      'archive-2',
    );
  });
});
