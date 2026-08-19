import { effect, Injectable, signal } from '@angular/core';

export type AppTheme = 'system' | 'light' | 'dark';
export type AppDensity = 'comfortable' | 'compact';
export type DefaultDestination = 'subfolder' | 'sameFolder' | 'ask';

interface StoredPreferences {
  theme: AppTheme;
  density: AppDensity;
  destination: DefaultDestination;
  preserveTree: boolean;
  showHidden: boolean;
}

const DEFAULTS: StoredPreferences = {
  theme: 'system',
  density: 'comfortable',
  destination: 'subfolder',
  preserveTree: true,
  showHidden: true,
};

@Injectable({ providedIn: 'root' })
export class PreferencesService {
  readonly theme = signal<AppTheme>(DEFAULTS.theme);
  readonly density = signal<AppDensity>(DEFAULTS.density);
  readonly destination = signal<DefaultDestination>(DEFAULTS.destination);
  readonly preserveTree = signal(DEFAULTS.preserveTree);
  readonly showHidden = signal(DEFAULTS.showHidden);

  constructor() {
    this.restore();
    effect(() => {
      const preferences: StoredPreferences = {
        theme: this.theme(),
        density: this.density(),
        destination: this.destination(),
        preserveTree: this.preserveTree(),
        showHidden: this.showHidden(),
      };
      this.apply(preferences);
      try {
        localStorage.setItem('fileflow.preferences', JSON.stringify(preferences));
      } catch {
        // Private browsing or locked-down environments can deny persistence.
      }
    });
  }

  private restore(): void {
    try {
      const stored = localStorage.getItem('fileflow.preferences');
      if (!stored) return;
      const parsed = JSON.parse(stored) as Partial<StoredPreferences>;
      if (parsed.theme === 'system' || parsed.theme === 'light' || parsed.theme === 'dark') this.theme.set(parsed.theme);
      if (parsed.density === 'comfortable' || parsed.density === 'compact') this.density.set(parsed.density);
      if (parsed.destination === 'subfolder' || parsed.destination === 'sameFolder' || parsed.destination === 'ask') this.destination.set(parsed.destination);
      if (typeof parsed.preserveTree === 'boolean') this.preserveTree.set(parsed.preserveTree);
      if (typeof parsed.showHidden === 'boolean') this.showHidden.set(parsed.showHidden);
    } catch {
      // Keep defaults if data is malformed.
    }
  }

  private apply(preferences: StoredPreferences): void {
    const root = document.documentElement;
    const resolvedTheme = preferences.theme === 'system'
      ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
      : preferences.theme;
    root.dataset['theme'] = resolvedTheme;
    root.dataset['density'] = preferences.density;
  }
}
