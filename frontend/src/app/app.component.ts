import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ActionDescriptor } from './core/ipc/tauri.models';
import { AuthStore } from './core/auth/auth.store';
import { CapabilityStore } from './core/catalog/capability.store';
import { PreferencesService } from './core/preferences/preferences.service';
import { WorkspaceStore } from './features/workspace/data-access/workspace.store';
import { UpdateService } from './core/update/update.service';

@Component({
  selector: 'ff-root',
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppComponent {
  private readonly destroyRef = inject(DestroyRef);
  private readonly router = inject(Router);
  protected readonly workspaceStore = inject(WorkspaceStore);
  protected readonly auth = inject(AuthStore);
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly preferences = inject(PreferencesService);
  protected readonly updater = inject(UpdateService);
  protected readonly paletteOpen = signal(false);
  protected readonly paletteQuery = signal('');
  private shellInitialized = false;
  private preferencesAccountId: string | null = null;

  protected readonly paletteActions = computed(() => {
    const query = this.paletteQuery().trim().toLowerCase();
    const actions = this.capabilities.actions();
    if (!query) return actions.filter((action) => action.featured).slice(0, 8);
    return actions
      .filter((action) => `${action.title} ${action.description}`.toLowerCase().includes(query))
      .slice(0, 10);
  });

  constructor() {
    effect(() => {
      const profileId = this.auth.profile()?.id ?? null;
      if (this.auth.authenticated() && this.auth.setupComplete() && profileId) {
        queueMicrotask(() => void this.initializeAuthenticatedContext(profileId));
      } else if (!this.auth.authenticated()) {
        this.preferencesAccountId = null;
      }
    });
    void this.initializeApplication();
  }

  private async initializeApplication(): Promise<void> {
    try {
      await this.auth.initialize();
      if (this.auth.needsWelcome()) {
        await this.router.navigate(['/welcome']);
        return;
      }
      if (this.router.url.startsWith('/welcome')) await this.router.navigate(['/']);
      const profileId = this.auth.profile()?.id;
      if (profileId) await this.initializeAuthenticatedContext(profileId);
    } finally {
      await this.revealDesktopWindow();
      // Update checks are deliberately non-blocking: startup/authentication must
      // remain instant even when GitHub or the update endpoint is unavailable.
      setTimeout(() => void this.updater.check(true), 2500);
    }
  }

  private async revealDesktopWindow(): Promise<void> {
    if (!isTauri()) return;
    try {
      const window = getCurrentWindow();
      await window.show();
      await window.setFocus();
    } catch (error) {
      // Keep bootstrap alive, but never hide startup-window failures:
      // a denied Tauri capability would otherwise leave the app invisible.
      console.error('[FileFlow] Unable to reveal main window', error);
    }
  }

  private async initializeAuthenticatedContext(profileId: string): Promise<void> {
    if (this.preferencesAccountId !== profileId) {
      this.preferencesAccountId = profileId;
      await this.preferences.reloadNative();
    }
    await this.initializeShellOnce();
    await this.capabilities.refreshUserData();
  }

  private async initializeShellOnce(): Promise<void> {
    if (this.shellInitialized) return;
    this.shellInitialized = true;
    await this.capabilities.initialize();
    await Promise.all([this.setupDesktopDrop(), this.setupDesktopNavigation()]);
  }

  @HostListener('document:keydown', ['$event'])
  protected onKeydown(event: KeyboardEvent): void {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      this.paletteOpen.update((open) => !open);
      if (!this.paletteOpen()) this.paletteQuery.set('');
      return;
    }
    if (event.key === 'Escape' && this.paletteOpen()) {
      this.closePalette();
    }
  }

  protected openPalette(): void {
    this.paletteOpen.set(true);
  }

  protected closePalette(): void {
    this.paletteOpen.set(false);
    this.paletteQuery.set('');
  }

  protected updatePaletteQuery(value: string): void {
    this.paletteQuery.set(value);
  }

  protected choosePaletteAction(action: ActionDescriptor): void {
    this.workspaceStore.setPendingAction(action.id);
    this.closePalette();
    if (this.workspaceStore.hasWorkspace()) {
      this.workspaceStore.openAction(action.id);
      void this.router.navigate(['/workspace']);
    } else {
      void this.router.navigate(['/']);
    }
  }

  protected runtimeLabel(): string {
    switch (this.capabilities.phase()) {
      case 'ready': return `${this.capabilities.engineReadyCount()}/${this.capabilities.engines().length} moteurs prêts`;
      case 'browser': return 'Aperçu navigateur';
      case 'error': return 'Moteur indisponible';
      default: return 'Initialisation…';
    }
  }

  private async setupDesktopNavigation(): Promise<void> {
    if (!isTauri()) return;

    const unlisten = await listen<string>('fileflow://navigate', (event) => {
      const path = event.payload;
      if (!path.startsWith('/')) return;
      void this.router.navigateByUrl(path);
    });

    this.destroyRef.onDestroy(() => unlisten());
  }

  private async setupDesktopDrop(): Promise<void> {
    if (!isTauri()) return;

    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      switch (event.payload.type) {
        case 'enter':
        case 'over':
          this.workspaceStore.setDragActive(true);
          break;
        case 'drop': {
          this.workspaceStore.setDragActive(false);
          const paths = event.payload.paths;
          if (paths.length > 0) {
            void this.router.navigate(['/workspace']);
            void this.workspaceStore.start(paths);
          }
          break;
        }
        case 'leave':
          this.workspaceStore.setDragActive(false);
          break;
      }
    });

    this.destroyRef.onDestroy(() => unlisten());
  }
}
