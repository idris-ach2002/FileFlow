import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  signal,
} from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { TauriBridgeService } from './core/ipc/tauri-bridge.service';
import { WorkspaceStore } from './features/workspace/data-access/workspace.store';

@Component({
  selector: 'ff-root',
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppComponent {
  private readonly bridge = inject(TauriBridgeService);
  private readonly destroyRef = inject(DestroyRef);
  private readonly router = inject(Router);
  protected readonly workspaceStore = inject(WorkspaceStore);
  protected readonly backendStatus = signal<'checking' | 'ready' | 'browser'>('checking');

  constructor() {
    void this.bridge.healthCheck().then(
      () => this.backendStatus.set('ready'),
      () => this.backendStatus.set('browser'),
    );
    void this.setupDesktopDrop();
  }

  private async setupDesktopDrop(): Promise<void> {
    if (!isTauri()) {
      return;
    }

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
