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
  private initialization: Promise<void> | null = null;

  readonly phase = signal<RuntimePhase>('loading');
  readonly health = signal<HealthResponse | null>(null);
  readonly engines = signal<EngineProbe[]>([]);
  readonly catalog = signal<CapabilityCatalog | null>(null);
  readonly executableActionIds = signal<ReadonlySet<string>>(new Set());
  readonly favoriteActionIds = signal<ReadonlySet<string>>(new Set());
  readonly error = signal<string | null>(null);

  readonly actions = computed(() => this.catalog()?.actions ?? []);
  readonly featuredActions = computed(() => {
    const favorites = this.favoriteActionIds();
    return this.actions()
      .filter((action) => action.featured)
      .sort((left, right) => Number(favorites.has(right.id)) - Number(favorites.has(left.id)));
  });
  readonly favoriteActions = computed(() => {
    const favorites = this.favoriteActionIds();
    return this.actions().filter((action) => favorites.has(action.id));
  });
  readonly availableEngineIds = computed(
    () => new Set(this.engines().filter((engine) => engine.available).map((engine) => engine.id)),
  );
  readonly engineReadyCount = computed(() => this.engines().filter((engine) => engine.available).length);
  readonly ready = computed(() => this.phase() === 'ready');

  initialize(): Promise<void> {
    this.initialization ??= this.initializeOnce();
    return this.initialization;
  }

  private async initializeOnce(): Promise<void> {
    if (!this.bridge.isDesktop()) {
      this.phase.set('browser');
      return;
    }
    try {
      const [health, engines, catalog, executableActions, favorites] = await Promise.all([
        this.bridge.healthCheck(),
        this.bridge.probeEngines(),
        this.bridge.capabilityCatalog(),
        this.bridge.executableActions(),
        this.bridge.favorites(),
      ]);
      this.health.set(health);
      this.engines.set(engines);
      this.catalog.set(catalog);
      this.executableActionIds.set(new Set(executableActions));
      this.favoriteActionIds.set(new Set(favorites));
      this.phase.set('ready');
      this.error.set(null);
    } catch (error) {
      this.error.set(errorMessage(error));
      this.phase.set('error');
    }
  }

  async refreshUserData(): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    try {
      this.favoriteActionIds.set(new Set(await this.bridge.favorites()));
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }

  action(id: string | null | undefined): ActionDescriptor | null {
    if (!id) return null;
    return this.actions().find((action) => action.id === id) ?? null;
  }

  isActionReady(action: ActionDescriptor): boolean {
    const engines = this.availableEngineIds();
    return action.requiredEngines.every((engine) => engines.has(engine));
  }

  isActionExecutable(action: ActionDescriptor): boolean {
    return this.isActionReady(action) && this.executableActionIds().has(action.id);
  }

  actionState(action: ActionDescriptor): 'ready' | 'missing-engine' | 'planned' {
    if (!this.isActionReady(action)) return 'missing-engine';
    return this.executableActionIds().has(action.id) ? 'ready' : 'planned';
  }


  isFavorite(actionId: string): boolean {
    return this.favoriteActionIds().has(actionId);
  }

  async toggleFavorite(actionId: string): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    const previous = this.favoriteActionIds();
    const next = new Set(previous);
    const favorite = !next.has(actionId);
    if (favorite) next.add(actionId); else next.delete(actionId);
    this.favoriteActionIds.set(next);
    try {
      await this.bridge.setFavorite(actionId, favorite);
    } catch (error) {
      this.favoriteActionIds.set(previous);
      throw error;
    }
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
