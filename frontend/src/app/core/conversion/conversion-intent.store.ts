import { Injectable, signal } from '@angular/core';
import { ActionUiKind, ActionInputMode } from '../ipc/tauri.models';

export interface ConversionIntent {
  actionId: string;
  sourceFormats: string[];
  strictSourceFormat?: boolean;
  targetFormat: string | null;
  inputMode: ActionInputMode;
  uiKind: ActionUiKind;
  parameters: Record<string, string | number | boolean | null>;
  createdAt: number;
}

const STORAGE_KEY = 'fileflow.conversion.intent.v1';

@Injectable({ providedIn: 'root' })
export class ConversionIntentStore {
  readonly intent = signal<ConversionIntent | null>(readStoredIntent());

  start(intent: Omit<ConversionIntent, 'createdAt'>): void {
    const value: ConversionIntent = { ...intent, createdAt: Date.now() };
    this.intent.set(value);
    try {
      sessionStorage.setItem(STORAGE_KEY, JSON.stringify(value));
    } catch {
      // The in-memory intent remains sufficient when session storage is blocked.
    }
  }

  forAction(actionId: string | null | undefined): ConversionIntent | null {
    if (!actionId) return null;
    const current = this.intent();
    return current?.actionId === actionId ? current : null;
  }

  clear(): void {
    this.intent.set(null);
    try { sessionStorage.removeItem(STORAGE_KEY); } catch { /* no-op */ }
  }
}

function readStoredIntent(): ConversionIntent | null {
  try {
    const value = sessionStorage.getItem(STORAGE_KEY);
    if (!value) return null;
    const parsed = JSON.parse(value) as ConversionIntent;
    if (!parsed?.actionId || Date.now() - Number(parsed.createdAt || 0) > 24 * 60 * 60 * 1000) return null;
    return parsed;
  } catch {
    return null;
  }
}
