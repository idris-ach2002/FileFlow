import { Injectable } from '@angular/core';

export interface WelcomeDraft {
  mode: 'create' | 'login';
  step: number;
  email: string;
  displayName: string;
  firstName: string;
  lastName: string;
  selectedAccountId?: string | null;
}

export interface GuidedFlowDraft {
  actionId?: string | null;
  targetFormat?: string | null;
  quality?: 'small' | 'balanced' | 'high';
  destination?: 'subfolder' | 'same' | 'choose';
  customDirectory?: string | null;
  finalCompression?: 'keep' | 'small' | 'balanced' | 'high';
  improve?: boolean;
  stripMetadata?: boolean;
  targetSizeMb?: number | null;
  signatureText?: string;
  collectionOrder?: 'name' | 'date' | 'selection';
  advancedOpen?: boolean;
}

@Injectable({ providedIn: 'root' })
export class UiMemoryService {
  private readonly prefix = 'fileflow.ui.v3.';

  welcomeDraft(): WelcomeDraft | null {
    return this.read<WelcomeDraft>('welcomeDraft');
  }

  saveWelcomeDraft(draft: WelcomeDraft): void {
    this.write('welcomeDraft', draft);
  }

  clearWelcomeDraft(): void {
    this.remove('welcomeDraft');
  }

  guidedFlowDraft(): GuidedFlowDraft | null {
    return this.read<GuidedFlowDraft>('guidedFlowDraft');
  }

  saveGuidedFlowDraft(draft: GuidedFlowDraft): void {
    this.write('guidedFlowDraft', draft);
  }

  clearGuidedFlowDraft(): void {
    this.remove('guidedFlowDraft');
  }


  workspaceRoots(): string[] {
    return this.read<string[]>('workspaceRoots') ?? [];
  }

  saveWorkspaceRoots(roots: string[]): void {
    this.write('workspaceRoots', [...new Set(roots.filter(Boolean))]);
  }

  clearWorkspaceRoots(): void {
    this.remove('workspaceRoots');
  }

  lastRoute(): string | null {
    const value = this.read<string>('lastRoute');
    return value && value !== '/welcome' ? value : null;
  }

  saveLastRoute(route: string): void {
    if (!route || route.startsWith('/welcome')) return;
    this.write('lastRoute', route);
  }

  private read<T>(key: string): T | null {
    try {
      const value = globalThis.localStorage?.getItem(this.prefix + key);
      return value ? JSON.parse(value) as T : null;
    } catch {
      return null;
    }
  }

  private write(key: string, value: unknown): void {
    try {
      globalThis.localStorage?.setItem(this.prefix + key, JSON.stringify(value));
    } catch {
      // Persistence is a convenience. Storage denial must never block FileFlow.
    }
  }

  private remove(key: string): void {
    try { globalThis.localStorage?.removeItem(this.prefix + key); } catch { /* best effort */ }
  }
}
