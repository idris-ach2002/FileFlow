import { Injectable, computed, signal } from '@angular/core';
import { getVersion } from '@tauri-apps/api/app';
import { isTauri } from '@tauri-apps/api/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';

type UpdateState = 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'current' | 'unavailable' | 'error';

@Injectable({ providedIn: 'root' })
export class UpdateService {
  private pending: Awaited<ReturnType<typeof check>> = null;
  readonly state = signal<UpdateState>('idle');
  readonly version = signal<string | null>(null);
  readonly currentVersion = signal<string | null>(null);
  readonly notes = signal<string | null>(null);
  readonly progress = signal(0);
  readonly downloadedBytes = signal(0);
  readonly totalBytes = signal(0);
  readonly message = signal<string | null>(null);
  readonly available = computed(() => this.state() === 'available');
  readonly configurationMissing = computed(() => this.state() === 'unavailable');
  readonly busy = computed(() => ['checking', 'downloading', 'installing'].includes(this.state()));
  readonly visible = computed(() => ['available', 'downloading', 'installing', 'error'].includes(this.state()));
  readonly statusLabel = computed(() => {
    switch (this.state()) {
      case 'idle': return this.message() ?? 'Prêt à rechercher une version stable.';
      case 'checking': return 'Recherche de la dernière version stable…';
      case 'available': return this.version() ? `Version ${this.version()} disponible` : 'Mise à jour disponible';
      case 'downloading': return this.totalBytes() > 0 ? `Téléchargement · ${this.progress()} %` : 'Téléchargement sécurisé…';
      case 'installing': return 'Installation signée en cours…';
      case 'current': return 'FileFlow est déjà à jour.';
      case 'unavailable': return this.message() ?? 'L’Updater n’est pas configuré pour ce build.';
      case 'error': return this.message() ?? 'La mise à jour n’a pas pu être installée.';
      default: return 'État de mise à jour inconnu.';
    }
  });

  constructor() {
    if (isTauri()) void getVersion().then((version) => this.currentVersion.set(version)).catch(() => undefined);
  }

  async check(silent = false): Promise<void> {
    if (!isTauri() || this.busy()) return;
    this.state.set('checking');
    this.message.set(null);
    this.pending = null;
    this.version.set(null);
    this.notes.set(null);
    try {
      const update = await check();
      this.pending = update;
      if (!update) {
        this.state.set('current');
        if (!silent) this.message.set('FileFlow est à jour.');
        return;
      }
      this.version.set(update.version);
      this.notes.set(update.body ?? null);
      this.state.set('available');
    } catch (error) {
      const rawMessage = error instanceof Error ? error.message : String(error);
      if (this.isConfigurationError(rawMessage)) {
        // A development build may deliberately have no updater key/endpoint.
        // Present an actionable status instead of leaking the plugin's raw
        // “Updater does not have any endpoints set” exception to the UI.
        this.state.set('unavailable');
        this.message.set('Updater non configuré pour ce build. Initialisez la signature puis reconstruisez FileFlow.');
        return;
      }
      if (silent) {
        // A transient network or GitHub failure must never interrupt startup or
        // leave a permanent red banner. The settings page can retry manually.
        this.state.set('idle');
        this.message.set(this.friendlyError(rawMessage));
        return;
      }
      this.state.set('error');
      this.message.set(this.friendlyError(rawMessage));
    }
  }

  async install(): Promise<void> {
    const update = this.pending;
    if (!update || this.busy()) return;
    this.progress.set(0);
    this.downloadedBytes.set(0);
    this.totalBytes.set(0);
    let downloaded = 0;
    let total = 0;
    this.state.set('downloading');
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0;
          this.totalBytes.set(total);
        }
        if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          this.downloadedBytes.set(downloaded);
          this.progress.set(total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0);
        }
        if (event.event === 'Finished') {
          this.progress.set(100);
          this.state.set('installing');
        }
      });
      this.message.set('Installation terminée. Redémarrage de FileFlow…');
      await relaunch();
    } catch (error) {
      this.state.set('error');
      this.message.set(this.friendlyError(error instanceof Error ? error.message : String(error)));
    }
  }

  dismiss(): void {
    if (!this.busy()) {
      this.state.set('idle');
      this.message.set(null);
    }
  }

  private isConfigurationError(message: string): boolean {
    return /does not have any endpoints|no updater endpoints?|endpoint.*not configured|pubkey|public key.*(?:missing|empty|configured)/i.test(message);
  }

  private friendlyError(message: string): string {
    if (/404|not found/i.test(message)) {
      return 'Aucune publication stable n’est encore disponible sur GitHub Releases.';
    }
    if (/network|dns|connect|timed? ?out|offline|fetch|request/i.test(message)) {
      return 'Impossible de joindre le service de mise à jour. Vérifiez la connexion puis réessayez.';
    }
    if (/signature|verify|verification/i.test(message)) {
      return 'La signature de cette mise à jour n’a pas pu être vérifiée. L’installation a été bloquée.';
    }
    return `La vérification a échoué : ${message}`;
  }
}
