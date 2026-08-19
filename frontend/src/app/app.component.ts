import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { TauriBridgeService } from './core/ipc/tauri-bridge.service';

@Component({
  selector: 'ff-root',
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  templateUrl: './app.component.html',
  styleUrl: './app.component.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AppComponent {
  private readonly bridge = inject(TauriBridgeService);
  protected readonly backendStatus = signal<'checking' | 'ready' | 'browser'>('checking');

  constructor() {
    void this.bridge.healthCheck().then(
      () => this.backendStatus.set('ready'),
      () => this.backendStatus.set('browser'),
    );
  }
}
