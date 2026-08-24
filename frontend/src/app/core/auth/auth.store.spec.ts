import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { TauriBridgeService } from '../ipc/tauri-bridge.service';
import { AuthSessionResponse } from '../ipc/tauri.models';
import { AuthStore } from './auth.store';

const session: AuthSessionResponse = {
  token: 'session-token',
  expiresAt: '2026-08-21T12:00:00Z',
  profile: {
    id: 'account-1',
    email: 'person@example.test',
    displayName: 'Person',
    firstName: 'Test',
    lastName: 'Person',
    avatarPath: '/tmp/avatar.png',
    createdAt: '2026-08-20T12:00:00Z',
    updatedAt: '2026-08-20T12:00:00Z',
  },
  onboarding: {
    accountId: 'account-1',
    completed: true,
    storageDirectory: '/tmp/FileFlow',
    language: 'fr',
    beginnerMode: true,
    preserveOriginals: true,
    notifications: true,
    confirmDestructiveActions: true,
    createdAt: '2026-08-20T12:00:00Z',
    updatedAt: '2026-08-20T12:00:00Z',
  },
};

describe('AuthStore', () => {
  it('does not keep login blocked while the avatar is loading', async () => {
    let resolveAvatar!: (value: { mimeType: string; bytes: number[] } | null) => void;
    const avatar = new Promise<{ mimeType: string; bytes: number[] } | null>((resolve) => {
      resolveAvatar = resolve;
    });
    const bridge = {
      isDesktop: vi.fn().mockReturnValue(true),
      login: vi.fn().mockResolvedValue(session),
      profileAvatar: vi.fn().mockReturnValue(avatar),
    };

    TestBed.configureTestingModule({
      providers: [AuthStore, { provide: TauriBridgeService, useValue: bridge }],
    });
    const store = TestBed.inject(AuthStore);

    const ok = await store.login({ email: session.profile.email, password: 'correct password' });

    expect(ok).toBe(true);
    expect(store.authenticated()).toBe(true);
    expect(store.profile()?.id).toBe('account-1');
    await Promise.resolve();
    expect(bridge.profileAvatar).toHaveBeenCalledWith('session-token');

    resolveAvatar(null);
    await Promise.resolve();
  });

  it('does not stack avatar pickers and always releases the busy state', async () => {
    let closeDialog!: (value: null) => void;
    const dialog = new Promise<null>((resolve) => { closeDialog = resolve; });
    const bridge = {
      isDesktop: vi.fn().mockReturnValue(true),
      chooseProfileAvatar: vi.fn().mockReturnValue(dialog),
    };

    TestBed.configureTestingModule({
      providers: [AuthStore, { provide: TauriBridgeService, useValue: bridge }],
    });
    const store = TestBed.inject(AuthStore);
    store.session.set(session);

    const first = store.chooseAvatar();
    const overlapping = store.chooseAvatar();

    expect(store.avatarBusy()).toBe(true);
    expect(bridge.chooseProfileAvatar).toHaveBeenCalledOnce();

    closeDialog(null);
    await Promise.all([first, overlapping]);
    expect(store.avatarBusy()).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('ignores an avatar response that arrives after logout', async () => {
    let finishAvatar!: (value: { mimeType: string; bytes: number[] }) => void;
    const avatar = new Promise<{ mimeType: string; bytes: number[] }>((resolve) => {
      finishAvatar = resolve;
    });
    const bridge = {
      isDesktop: vi.fn().mockReturnValue(true),
      profileAvatar: vi.fn().mockReturnValue(avatar),
      logout: vi.fn().mockResolvedValue(true),
    };

    TestBed.configureTestingModule({
      providers: [AuthStore, { provide: TauriBridgeService, useValue: bridge }],
    });
    const store = TestBed.inject(AuthStore);
    store.session.set(session);

    const loading = store.loadAvatar();
    await store.logout();
    finishAvatar({ mimeType: 'image/png', bytes: [137, 80, 78, 71] });
    await loading;

    expect(store.avatarUrl()).toBeNull();
    expect(store.phase()).toBe('signedOut');
  });
});
