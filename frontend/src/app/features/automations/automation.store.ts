import { inject, Injectable, signal } from '@angular/core';
import { TauriBridgeService } from '../../core/ipc/tauri-bridge.service';
import { RecipeRecord } from '../../core/ipc/tauri.models';

@Injectable({ providedIn: 'root' })
export class AutomationStore {
  private readonly bridge = inject(TauriBridgeService);
  readonly recipes = signal<RecipeRecord[]>([]);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);

  load(): void {
    if (!this.bridge.isDesktop() || this.loading()) return;
    this.loading.set(true);
    void this.bridge.recipes().then(
      (recipes) => { this.recipes.set(recipes); this.loading.set(false); },
      (error: unknown) => { this.error.set(message(error)); this.loading.set(false); },
    );
  }

  async save(recipe: RecipeRecord): Promise<boolean> {
    this.error.set(null);
    try {
      await this.bridge.saveRecipe(recipe);
      this.load();
      return true;
    } catch (error) {
      this.error.set(message(error));
      return false;
    }
  }
}

function message(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Impossible d’enregistrer la recette.';
}
