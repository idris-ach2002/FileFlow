import { Injectable, computed, inject, signal } from '@angular/core';
import {
  AccountProfile,
  AuthSessionResponse,
  ChangePasswordRequest,
  CreateAccountRequest,
  LoginRequest,
  OnboardingPreferences,
  ProfileUpdate,
} from '../ipc/tauri.models';
import { TauriBridgeService } from '../ipc/tauri-bridge.service';

export type AuthPhase = 'loading' | 'signedOut' | 'authenticated' | 'error';

@Injectable({ providedIn: 'root' })
export class AuthStore {
  private readonly bridge = inject(TauriBridgeService);
  readonly phase = signal<AuthPhase>('loading');
  readonly hasAccount = signal(false);
  readonly session = signal<AuthSessionResponse | null>(null);
  readonly error = signal<string | null>(null);
  readonly avatarUrl = signal<string | null>(null);
  private initialization: Promise<void> | null = null;

  readonly authenticated = computed(() => this.phase() === 'authenticated' && this.session() !== null);
  readonly profile = computed(() => this.session()?.profile ?? null);
  readonly onboarding = computed(() => this.session()?.onboarding ?? null);
  readonly setupComplete = computed(() => this.onboarding()?.completed ?? false);
  readonly needsWelcome = computed(() => !this.authenticated() || !this.setupComplete());
  readonly initials = computed(() => {
    const profile = this.profile();
    if (!profile) return 'FF';
    const pieces = [profile.firstName, profile.lastName, profile.displayName]
      .filter(Boolean)
      .flatMap((value) => value.trim().split(/\s+/))
      .filter(Boolean);
    return pieces.slice(0, 2).map((value) => value[0]?.toUpperCase() ?? '').join('') || 'FF';
  });

  initialize(): Promise<void> {
    this.initialization ??= this.initializeOnce();
    return this.initialization;
  }

  private async initializeOnce(): Promise<void> {
    this.phase.set('loading');
    this.error.set(null);
    if (!this.bridge.isDesktop()) {
      this.session.set(this.browserPreviewSession());
      this.hasAccount.set(true);
      this.phase.set('authenticated');
      return;
    }
    try {
      const bootstrap = await this.bridge.accountBootstrap();
      this.hasAccount.set(bootstrap.hasAccount);
      this.phase.set('signedOut');
    } catch (error) {
      this.error.set(this.message(error));
      this.phase.set('error');
    }
  }

  async createAccount(request: CreateAccountRequest): Promise<boolean> {
    return this.runAuth(() => this.bridge.createAccount(request));
  }

  async login(request: LoginRequest): Promise<boolean> {
    return this.runAuth(() => this.bridge.login(request));
  }

  async changePassword(request: ChangePasswordRequest): Promise<boolean> {
    const session = this.session();
    if (!session) return false;
    this.error.set(null);
    try {
      if (!this.bridge.isDesktop()) return true;
      const refreshed = await this.bridge.changePassword(session.token, request);
      this.session.set(refreshed);
      return true;
    } catch (error) {
      this.error.set(this.message(error));
      return false;
    }
  }

  async logout(): Promise<void> {
    const token = this.session()?.token;
    if (token && this.bridge.isDesktop()) {
      try { await this.bridge.logout(token); } catch { /* session is local and may already be gone */ }
    }
    this.revokeAvatarUrl();
    this.session.set(null);
    this.phase.set('signedOut');
  }

  async saveSetup(update: Partial<OnboardingPreferences>, completed?: boolean): Promise<boolean> {
    const session = this.session();
    if (!session) return false;
    const value: OnboardingPreferences = {
      ...session.onboarding,
      ...update,
      completed: completed ?? update.completed ?? session.onboarding.completed,
      updatedAt: new Date().toISOString(),
    };
    try {
      const onboarding = this.bridge.isDesktop()
        ? await this.bridge.saveOnboarding(session.token, value)
        : value;
      this.session.set({ ...session, onboarding });
      this.error.set(null);
      return true;
    } catch (error) {
      this.error.set(this.message(error));
      return false;
    }
  }

  async updateProfile(request: ProfileUpdate): Promise<boolean> {
    const session = this.session();
    if (!session) return false;
    try {
      const profile = this.bridge.isDesktop()
        ? await this.bridge.updateProfile(session.token, request)
        : { ...session.profile, ...request, updatedAt: new Date().toISOString() };
      this.session.set({ ...session, profile });
      this.error.set(null);
      return true;
    } catch (error) {
      this.error.set(this.message(error));
      return false;
    }
  }

  async chooseAvatar(): Promise<void> {
    const session = this.session();
    if (!session || !this.bridge.isDesktop()) return;
    try {
      const profile = await this.bridge.chooseProfileAvatar(session.token);
      if (!profile) return;
      this.session.set({ ...session, profile });
      await this.loadAvatar();
    } catch (error) {
      this.error.set(this.message(error));
    }
  }

  async loadAvatar(): Promise<void> {
    const session = this.session();
    if (!session || !session.profile.avatarPath || !this.bridge.isDesktop()) {
      this.revokeAvatarUrl();
      return;
    }
    try {
      const payload = await this.bridge.profileAvatar(session.token);
      if (!payload) return;
      this.revokeAvatarUrl();
      const blob = new Blob([new Uint8Array(payload.bytes)], { type: payload.mimeType });
      this.avatarUrl.set(URL.createObjectURL(blob));
    } catch {
      this.revokeAvatarUrl();
    }
  }

  defaultStorageDirectory(): Promise<string> {
    if (!this.bridge.isDesktop()) return Promise.resolve('~/Documents/FileFlow');
    return this.bridge.defaultStorageDirectory();
  }

  chooseStorageDirectory(): Promise<string | null> {
    if (!this.bridge.isDesktop()) return Promise.resolve(null);
    return this.bridge.chooseStorageDirectory();
  }

  clearError(): void { this.error.set(null); }

  private async runAuth(operation: () => Promise<AuthSessionResponse>): Promise<boolean> {
    this.error.set(null);
    try {
      const session = await operation();
      this.session.set(session);
      this.hasAccount.set(true);
      this.phase.set('authenticated');
      await this.loadAvatar();
      return true;
    } catch (error) {
      this.error.set(this.message(error));
      return false;
    }
  }

  private revokeAvatarUrl(): void {
    const current = this.avatarUrl();
    if (current?.startsWith('blob:')) URL.revokeObjectURL(current);
    this.avatarUrl.set(null);
  }

  private message(error: unknown): string {
    return error instanceof Error ? error.message : String(error || 'Une erreur est survenue.');
  }

  private browserPreviewSession(): AuthSessionResponse {
    const now = new Date().toISOString();
    const profile: AccountProfile = {
      id: 'preview', email: 'preview@fileflow.local', displayName: 'Compte démo',
      firstName: 'Compte', lastName: 'démo', createdAt: now, updatedAt: now,
    };
    const onboarding: OnboardingPreferences = {
      accountId: profile.id, completed: true, storageDirectory: '~/Documents/FileFlow', language: 'fr',
      beginnerMode: true, preserveOriginals: true, notifications: true,
      confirmDestructiveActions: true, createdAt: now, updatedAt: now,
    };
    return { token: 'browser-preview', expiresAt: now, profile, onboarding };
  }
}
