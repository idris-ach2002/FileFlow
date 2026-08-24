import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  getVersion: vi.fn().mockResolvedValue('1.0.6'),
  relaunch: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@tauri-apps/api/app', () => ({ getVersion: mocks.getVersion }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: mocks.relaunch }));
vi.mock('@tauri-apps/plugin-updater', () => ({ check: mocks.check }));

import { UpdateService } from './update.service';

describe('UpdateService', () => {
  beforeEach(() => {
    mocks.check.mockReset();
    mocks.relaunch.mockClear();
  });

  it('reports an unpublished manifest instead of a connection outage', async () => {
    mocks.check.mockRejectedValueOnce('Could not fetch a valid release JSON from the remote');
    const service = new UpdateService();

    await service.check(false);

    expect(service.state()).toBe('error');
    expect(service.statusLabel()).toContain('latest.json');
    expect(service.statusLabel()).not.toContain('connexion');
  });

  it('reports that the installed version is current when no update exists', async () => {
    mocks.check.mockResolvedValueOnce(null);
    const service = new UpdateService();

    await service.check(false);

    expect(service.state()).toBe('current');
  });

  it('downloads, installs and relaunches an available signed update', async () => {
    const downloadAndInstall = vi.fn(async (listener: (event: unknown) => void) => {
      listener({ event: 'Started', data: { contentLength: 100 } });
      listener({ event: 'Progress', data: { chunkLength: 40 } });
      listener({ event: 'Progress', data: { chunkLength: 60 } });
      listener({ event: 'Finished', data: {} });
    });
    mocks.check.mockResolvedValueOnce({ version: '1.0.7', body: 'Correctifs', downloadAndInstall });
    const service = new UpdateService();

    await service.check(false);
    expect(service.state()).toBe('available');
    expect(service.version()).toBe('1.0.7');

    await service.install();

    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(service.progress()).toBe(100);
    expect(service.downloadedBytes()).toBe(100);
    expect(mocks.relaunch).toHaveBeenCalledOnce();
  });
});
