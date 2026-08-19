import { computed, inject, Injectable, signal } from '@angular/core';
import { TauriBridgeService } from '../ipc/tauri-bridge.service';
import {
  ActionDescriptor,
  CapabilityCatalog,
  EngineProbe,
  HealthResponse,
} from '../ipc/tauri.models';

export type RuntimePhase = 'loading' | 'ready' | 'browser' | 'error';

@Injectable({ providedIn: 'root' })
export class CapabilityStore {
  private readonly bridge = inject(TauriBridgeService);
  private initialized = false;

  readonly phase = signal<RuntimePhase>('loading');
  readonly health = signal<HealthResponse | null>(null);
  readonly engines = signal<EngineProbe[]>([]);
  readonly catalog = signal<CapabilityCatalog | null>(null);
  readonly error = signal<string | null>(null);

  readonly actions = computed(() => this.catalog()?.actions ?? []);
  readonly featuredActions = computed(() => this.actions().filter((action) => action.featured));
  readonly availableEngineIds = computed(
    () => new Set(this.engines().filter((engine) => engine.available).map((engine) => engine.id)),
  );
  readonly engineReadyCount = computed(() => this.engines().filter((engine) => engine.available).length);
  readonly ready = computed(() => this.phase() === 'ready');

  initialize(): void {
    if (this.initialized) return;
    this.initialized = true;

    if (!this.bridge.isDesktop()) {
      this.phase.set('browser');
      return;
    }

    void Promise.all([
      this.bridge.healthCheck(),
      this.bridge.probeEngines(),
      this.bridge.capabilityCatalog(),
    ]).then(
      ([health, engines, catalog]) => {
        this.health.set(health);
        this.engines.set(engines);
        this.catalog.set(catalog);
        this.phase.set('ready');
      },
      (error: unknown) => {
        this.error.set(errorMessage(error));
        this.phase.set('error');
      },
    );
  }

  action(id: string | null | undefined): ActionDescriptor | null {
    if (!id) return null;
    return this.actions().find((action) => action.id === id) ?? null;
  }

  isActionReady(action: ActionDescriptor): boolean {
    const engines = this.availableEngineIds();
    return action.requiredEngines.every((engine) => engines.has(engine));
  }

  missingEngines(action: ActionDescriptor): string[] {
    const engines = this.availableEngineIds();
    return action.requiredEngines.filter((engine) => !engines.has(engine));
  }

  async refreshEngines(): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    this.engines.set(await this.bridge.probeEngines());
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Le moteur FileFlow n’a pas pu être initialisé.';
}
