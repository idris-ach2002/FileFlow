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
});
