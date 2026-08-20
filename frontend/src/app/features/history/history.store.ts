import { computed, inject, Injectable, signal } from '@angular/core';
import { TauriBridgeService } from '../../core/ipc/tauri-bridge.service';
import { HistoryEntry } from '../../core/ipc/tauri.models';

@Injectable({ providedIn: 'root' })
export class HistoryStore {
  private readonly bridge = inject(TauriBridgeService);
  private loaded = false;

  readonly entries = signal<HistoryEntry[]>([]);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly totalInputBytes = computed(() => this.entries().reduce((sum, entry) => sum + entry.inputBytes, 0));
  readonly totalOutputBytes = computed(() => this.entries().reduce((sum, entry) => sum + entry.outputBytes, 0));
  readonly savedBytes = computed(() => this.entries().reduce((sum, entry) => {
    if (!isSizeReductionAction(entry.actionId) || entry.status !== 'completed') return sum;
    return sum + Math.max(0, entry.inputBytes - entry.outputBytes);
  }, 0));
  readonly completed = computed(() => this.entries().filter((entry) => entry.status === 'completed').length);

  load(force = false): void {
    if ((this.loaded && !force) || this.loading()) return;
    if (!this.bridge.isDesktop()) return;
    this.loading.set(true);
    this.error.set(null);
    void this.bridge.history(200).then(
      (entries) => {
        this.entries.set(entries);
        this.loaded = true;
        this.loading.set(false);
      },
      (error: unknown) => {
        this.error.set(errorMessage(error));
        this.loading.set(false);
      },
    );
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Impossible de charger l’historique local.';
}

function isSizeReductionAction(actionId: string): boolean {
  return actionId === 'pdf-compress' || actionId === 'media-compress' || actionId === 'image-optimize';
}
