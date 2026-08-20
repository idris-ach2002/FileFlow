import { Injectable, computed, signal } from '@angular/core';
import { isTauri } from '@tauri-apps/api/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';

type UpdateState = 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'current' | 'unavailable' | 'error';

@Injectable({ providedIn: 'root' })
export class UpdateService {
  private pending: Awaited<ReturnType<typeof check>> = null;
  readonly state = signal<UpdateState>('idle');
  readonly version = signal<string | null>(null);
  readonly progress = signal(0);
  readonly message = signal<string | null>(null);
  readonly available = computed(() => this.state() === 'available');
  readonly busy = computed(() => ['checking', 'downloading', 'installing'].includes(this.state()));

  async check(silent = false): Promise<void> {
    if (!isTauri() || this.busy()) return;
    this.state.set('checking');
    this.message.set(null);
    try {
      const update = await check();
      this.pending = update;
      if (!update) {
        this.state.set('current');
        if (!silent) this.message.set('FileFlow est à jour.');
        return;
      }
      this.version.set(update.version);
      this.state.set('available');
    } catch (error) {
      // Development builds and unsigned/private channels may intentionally have
      // no updater endpoint. Do not turn this into a startup failure.
      this.state.set(silent ? 'unavailable' : 'error');
      if (!silent) this.message.set(error instanceof Error ? error.message : String(error));
    }
  }

  async install(): Promise<void> {
    const update = this.pending;
    if (!update || this.busy()) return;
    this.progress.set(0);
    let downloaded = 0;
    let total = 0;
    this.state.set('downloading');
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') total = event.data.contentLength ?? 0;
        if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          this.progress.set(total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0);
        }
        if (event.event === 'Finished') this.state.set('installing');
      });
      await relaunch();
    } catch (error) {
      this.state.set('error');
      this.message.set(error instanceof Error ? error.message : String(error));
    }
  }

  dismiss(): void {
    if (!this.busy()) this.state.set('idle');
  }
}
