import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  HostListener,
  computed,
  inject,
  signal,
} from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { ActionDescriptor } from './core/ipc/tauri.models';
import { CapabilityStore } from './core/catalog/capability.store';
import { PreferencesService } from './core/preferences/preferences.service';
import { WorkspaceStore } from './features/workspace/data-access/workspace.store';

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
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly preferences = inject(PreferencesService);
  protected readonly paletteOpen = signal(false);
  protected readonly paletteQuery = signal('');

  protected readonly paletteActions = computed(() => {
    const query = this.paletteQuery().trim().toLowerCase();
    const actions = this.capabilities.actions();
    if (!query) return actions.filter((action) => action.featured).slice(0, 8);
    return actions
      .filter((action) => `${action.title} ${action.description}`.toLowerCase().includes(query))
      .slice(0, 10);
  });

  constructor() {
    this.capabilities.initialize();
    void this.setupDesktopDrop();
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
