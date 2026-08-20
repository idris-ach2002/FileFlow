import { effect, inject, Injectable, signal } from '@angular/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { TauriBridgeService } from '../ipc/tauri-bridge.service';

export type AppTheme = 'system' | 'light' | 'dark';
export type AppDensity = 'comfortable' | 'compact';
export type DefaultDestination = 'subfolder' | 'sameFolder' | 'ask';
export type DefaultQuality = 'small' | 'balanced' | 'high';
export type PerformanceMode = 'eco' | 'balanced' | 'fast';
export type AppLanguage = 'fr' | 'en' | 'de';

export interface StoredPreferences {
  theme: AppTheme;
  density: AppDensity;
  destination: DefaultDestination;
  preserveTree: boolean;
  showHidden: boolean;
  beginnerMode: boolean;
  uiScale: number;
  reduceMotion: boolean;
  language: AppLanguage;
  confirmDestructive: boolean;
  notifyOnCompletion: boolean;
  autoOpenResults: boolean;
  defaultQuality: DefaultQuality;
  performanceMode: PerformanceMode;
  showTechnicalDetails: boolean;
}

const STORAGE_KEY = 'fileflow.preferences.v2';
const DEFAULTS: StoredPreferences = {
  theme: 'system', density: 'comfortable', destination: 'subfolder', preserveTree: true,
  showHidden: true, beginnerMode: true, uiScale: 1, reduceMotion: false, language: 'fr',
  confirmDestructive: true, notifyOnCompletion: true, autoOpenResults: false,
  defaultQuality: 'balanced', performanceMode: 'balanced', showTechnicalDetails: false,
};

@Injectable({ providedIn: 'root' })
export class PreferencesService {
  private readonly bridge = inject(TauriBridgeService);
  private nativeSaveTimer: ReturnType<typeof setTimeout> | null = null;
  private restoringNative = false;
  private appliedPerformanceMode: PerformanceMode | null = null;

  readonly theme = signal<AppTheme>(DEFAULTS.theme);
  readonly density = signal<AppDensity>(DEFAULTS.density);
  readonly destination = signal<DefaultDestination>(DEFAULTS.destination);
  readonly preserveTree = signal(DEFAULTS.preserveTree);
  readonly showHidden = signal(DEFAULTS.showHidden);
  readonly beginnerMode = signal(DEFAULTS.beginnerMode);
  readonly uiScale = signal(DEFAULTS.uiScale);
  readonly reduceMotion = signal(DEFAULTS.reduceMotion);
  readonly language = signal<AppLanguage>(DEFAULTS.language);
  readonly confirmDestructive = signal(DEFAULTS.confirmDestructive);
  readonly notifyOnCompletion = signal(DEFAULTS.notifyOnCompletion);
  readonly autoOpenResults = signal(DEFAULTS.autoOpenResults);
  readonly defaultQuality = signal<DefaultQuality>(DEFAULTS.defaultQuality);
  readonly performanceMode = signal<PerformanceMode>(DEFAULTS.performanceMode);
  readonly showTechnicalDetails = signal(DEFAULTS.showTechnicalDetails);

  constructor() {
    this.restoreLocal();
    void this.reloadNative();
    effect(() => {
      const preferences = this.snapshot();
      this.apply(preferences);
      try { localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences)); } catch { /* best-effort cache */ }
      if (!this.restoringNative) this.scheduleNativeSave(preferences);
    });
  }

  reset(): void { this.load(DEFAULTS); }

  private snapshot(): StoredPreferences {
    return {
      theme: this.theme(), density: this.density(), destination: this.destination(),
      preserveTree: this.preserveTree(), showHidden: this.showHidden(), beginnerMode: this.beginnerMode(),
      uiScale: this.uiScale(), reduceMotion: this.reduceMotion(), language: this.language(),
      confirmDestructive: this.confirmDestructive(), notifyOnCompletion: this.notifyOnCompletion(),
      autoOpenResults: this.autoOpenResults(), defaultQuality: this.defaultQuality(),
      performanceMode: this.performanceMode(), showTechnicalDetails: this.showTechnicalDetails(),
    };
  }

  private restoreLocal(): void {
    try {
      const stored = localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem('fileflow.preferences');
      if (stored) this.load(JSON.parse(stored) as Partial<StoredPreferences>);
    } catch { /* malformed cache: keep safe defaults */ }
  }

  async reloadNative(): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    this.restoringNative = true;
    try {
      const stored = await this.bridge.loadPreferences();
      if (stored) this.load(stored as Partial<StoredPreferences>);
    } catch { /* local cache remains a valid fallback */ }
    finally { this.restoringNative = false; }
  }

  private load(value: Partial<StoredPreferences>): void {
    if (isOneOf(value.theme, ['system','light','dark'])) this.theme.set(value.theme);
    if (isOneOf(value.density, ['comfortable','compact'])) this.density.set(value.density);
    if (isOneOf(value.destination, ['subfolder','sameFolder','ask'])) this.destination.set(value.destination);
    if (typeof value.preserveTree === 'boolean') this.preserveTree.set(value.preserveTree);
    if (typeof value.showHidden === 'boolean') this.showHidden.set(value.showHidden);
    if (typeof value.beginnerMode === 'boolean') this.beginnerMode.set(value.beginnerMode);
    if (typeof value.uiScale === 'number') this.uiScale.set(Math.min(1.4, Math.max(.8, value.uiScale)));
    if (typeof value.reduceMotion === 'boolean') this.reduceMotion.set(value.reduceMotion);
    if (isOneOf(value.language, ['fr','en','de'])) this.language.set(value.language);
    if (typeof value.confirmDestructive === 'boolean') this.confirmDestructive.set(value.confirmDestructive);
    if (typeof value.notifyOnCompletion === 'boolean') this.notifyOnCompletion.set(value.notifyOnCompletion);
    if (typeof value.autoOpenResults === 'boolean') this.autoOpenResults.set(value.autoOpenResults);
    if (isOneOf(value.defaultQuality, ['small','balanced','high'])) this.defaultQuality.set(value.defaultQuality);
    if (isOneOf(value.performanceMode, ['eco','balanced','fast'])) this.performanceMode.set(value.performanceMode);
    if (typeof value.showTechnicalDetails === 'boolean') this.showTechnicalDetails.set(value.showTechnicalDetails);
  }

  private apply(preferences: StoredPreferences): void {
    const root = document.documentElement;
    const resolvedTheme = preferences.theme === 'system'
      ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : preferences.theme;
    root.dataset['theme'] = resolvedTheme;
    root.dataset['density'] = preferences.density;
    root.dataset['motion'] = preferences.reduceMotion ? 'reduced' : 'full';
    root.dataset['experience'] = preferences.beginnerMode ? 'guided' : 'advanced';
    if (this.bridge.isDesktop()) {
      void getCurrentWebview().setZoom(preferences.uiScale).catch(() => undefined);
      if (this.appliedPerformanceMode !== preferences.performanceMode) {
        this.appliedPerformanceMode = preferences.performanceMode;
        void this.bridge.setPerformanceMode(preferences.performanceMode).catch(() => {
          this.appliedPerformanceMode = null;
        });
      }
    }
  }

  private scheduleNativeSave(preferences: StoredPreferences): void {
    if (!this.bridge.isDesktop()) return;
    if (this.nativeSaveTimer) clearTimeout(this.nativeSaveTimer);
    this.nativeSaveTimer = setTimeout(() => {
      void this.bridge.savePreferences(preferences as unknown as Record<string, unknown>).catch(() => undefined);
    }, 180);
  }
}

function isOneOf<T extends string>(value: unknown, allowed: readonly T[]): value is T {
  return typeof value === 'string' && allowed.includes(value as T);
}
