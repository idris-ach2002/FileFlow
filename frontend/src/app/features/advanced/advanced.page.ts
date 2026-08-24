import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ConversionIntentStore } from '../../core/conversion/conversion-intent.store';
import { ActionDescriptor, ActionParameterDescriptor, ActionUiKind } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

type AdvancedGroup = 'all' | 'conversion' | 'pdf' | 'image' | 'document' | 'media' | 'archive' | 'organization' | 'privacy';

interface GroupItem { id: AdvancedGroup; label: string; icon: string; }

@Component({
  selector: 'ff-advanced-page',
  template: `
    <div class="ff-page advanced-shell">
      <header class="advanced-hero">
        <div>
          <div class="expert-badge"><span>✦</span> ESPACE EXPERT</div>
          <h1 class="ff-title">Choisissez précisément votre opération.</h1>
          <p class="ff-subtitle">Chaque outil ouvre ensuite l’onglet Conversion avec les bons formats, les fichiers compatibles et ses réglages spécifiques.</p>
        </div>
        <div class="health-card">
          <span class="health-orb">{{ capabilities.engineReadyCount() }}</span>
          <div><strong>moteurs disponibles</strong><small>sur {{ capabilities.engines().length }} détectés</small></div>
          <span class="ff-badge success">● Traitement local</span>
        </div>
      </header>

      <section class="advanced-layout ff-section">
        <aside class="group-rail ff-panel" aria-label="Familles d’opérations">
          <p class="ff-kicker">OPÉRATIONS</p>
          @for (group of groups; track group.id) {
            <button type="button" [class.active]="selectedGroup() === group.id" (click)="selectedGroup.set(group.id)">
              <span>{{ group.icon }}</span><strong>{{ group.label }}</strong><small>{{ groupCount(group.id) }}</small>
            </button>
          }
          <div class="rail-separator"></div>
          <button type="button" (click)="router.navigate(['/automations'])"><span>⚡</span><strong>Automatisations</strong><small>↗</small></button>
          <button type="button" (click)="router.navigate(['/formats'])"><span>◇</span><strong>Formats</strong><small>↗</small></button>
          <button type="button" (click)="router.navigate(['/settings'], { queryParams: { section: 'engines' } })"><span>▣</span><strong>Moteurs</strong><small>↗</small></button>
        </aside>

        <main class="catalog ff-panel">
          <header class="catalog-head">
            <div><p class="ff-kicker">CATALOGUE AVANCÉ</p><h2>{{ groupTitle() }}</h2><p>{{ filteredActions().length }} action(s) · paramètres et formats fournis par le moteur FileFlow.</p></div>
            <label class="catalog-search"><span>⌕</span><input #query type="search" placeholder="OCR, WebP, rotation, TAR.ZST…" [value]="search()" (input)="search.set(query.value)" /></label>
          </header>

          @if (selectedGroup() === 'conversion') {
            <section class="conversion-composer">
              <div><span class="composer-mark">↻</span><div><strong>Conversion avancée</strong><small>Définissez le format de départ et la cible avant de sélectionner les fichiers.</small></div></div>
              <label><span>Format de départ</span><select [value]="conversionSource()" (change)="setConversionSource($any($event.target).value)">@for (format of conversionSources(); track format.id) { <option [value]="format.id">{{ format.label }} ({{ format.id.toUpperCase() }})</option> }</select></label>
              <span class="composer-arrow">→</span>
              <label><span>Format cible</span><select [value]="conversionTarget()" (change)="conversionTarget.set($any($event.target).value)">@for (target of conversionTargets(); track target) { <option [value]="target">{{ target.toUpperCase() }}</option> }</select></label>
              <button type="button" [disabled]="!conversionTarget()" (click)="launchFormatConversion()">Préparer <span>→</span></button>
            </section>
          }

          <div class="catalog-toolbar">
            <button type="button" [class.active]="availability() === 'all'" (click)="availability.set('all')">Toutes</button>
            <button type="button" [class.active]="availability() === 'ready'" (click)="availability.set('ready')">Prêtes</button>
            <button type="button" [class.active]="availability() === 'favorites'" (click)="availability.set('favorites')">Favorites</button>
            <span>{{ capabilities.formats().length }} profils de format</span>
          </div>

          <div class="catalog-grid">
            @for (action of filteredActions(); track action.id) {
              <article class="action-card" [class.unavailable]="!capabilities.isActionExecutable(action)" [attr.data-kind]="uiKind(action)">
                <div class="action-top">
                  <span class="action-mark">{{ actionMark(action) }}</span>
                  <span class="kind-label">{{ uiKindLabel(action) }}</span>
                  <button class="star" type="button" (click)="toggleFavorite(action)" [attr.aria-label]="capabilities.isFavorite(action.id) ? 'Retirer des favoris' : 'Ajouter aux favoris'">{{ capabilities.isFavorite(action.id) ? '★' : '☆' }}</button>
                </div>
                <h3>{{ action.title }}</h3>
                <p>{{ action.description }}</p>
                <div class="action-meta">
                  @if (sourceSummary(action); as source) { <span><b>Entrée</b>{{ source }}</span> }
                  @if (targetSummary(action); as target) { <span><b>Sortie</b>{{ target }}</span> }
                  @if (parameterCount(action)) { <span><b>Réglages</b>{{ parameterCount(action) }}</span> }
                </div>
                @if (!capabilities.isActionReady(action)) { <div class="missing">Moteur requis : {{ capabilities.missingEngines(action).join(', ') }}</div> }
                <button class="open-action" type="button" [disabled]="!capabilities.isActionExecutable(action)" (click)="startAction(action)">
                  {{ capabilities.actionState(action) === 'planned' ? 'Fonction planifiée' : capabilities.isActionReady(action) ? 'Configurer dans Conversion' : 'Moteur absent' }} <span>→</span>
                </button>
              </article>
            } @empty {
              <div class="catalog-empty"><span>⌕</span><strong>Aucune action trouvée</strong><small>Modifiez la famille, le filtre ou la recherche.</small></div>
            }
          </div>
        </main>
      </section>
    </div>
  `,
  styles: [`
    :host{display:block}.advanced-shell{padding-bottom:24px}.advanced-hero{display:grid;grid-template-columns:minmax(0,1fr) 290px;gap:28px;align-items:end}.expert-badge{display:inline-flex;align-items:center;gap:8px;margin-bottom:14px;padding:7px 10px;border:1px solid color-mix(in srgb,var(--accent) 14%,var(--border));border-radius:999px;background:var(--accent-soft);color:var(--accent-strong);font-size:10px;font-weight:900;letter-spacing:.1em}.advanced-hero .ff-title{max-width:790px}.health-card{min-height:120px;display:grid;grid-template-columns:64px minmax(0,1fr);align-items:center;gap:12px;padding:18px;border:1px solid var(--border);border-radius:20px;background:linear-gradient(145deg,var(--surface-1),var(--accent-soft-2));box-shadow:var(--shadow-sm)}.health-orb{width:62px;height:62px;display:grid;place-items:center;grid-row:1/3;border-radius:20px;background:linear-gradient(145deg,var(--accent),var(--violet));color:white;font-size:24px;font-weight:900}.health-card strong,.health-card small{display:block}.health-card small{margin-top:3px;color:var(--text-muted);font-size:11px}.health-card .ff-badge{width:max-content}.advanced-layout{display:grid;grid-template-columns:230px minmax(0,1fr);gap:14px;align-items:start}.group-rail{position:sticky;top:20px;padding:12px}.group-rail>.ff-kicker{padding:5px 8px}.group-rail button{width:100%;min-height:43px;display:grid;grid-template-columns:28px minmax(0,1fr) auto;align-items:center;gap:8px;padding:0 9px;border:0;border-radius:11px;background:transparent;color:var(--text-muted);text-align:left}.group-rail button:hover,.group-rail button.active{background:var(--accent-soft);color:var(--accent)}.group-rail button span{font-size:15px;text-align:center}.group-rail button strong{font-size:11px}.group-rail button small{font-size:9px}.rail-separator{height:1px;margin:9px;background:var(--border)}.catalog{padding:22px}.catalog-head{display:grid;grid-template-columns:minmax(0,1fr) minmax(270px,390px);gap:18px;align-items:end;padding-bottom:17px;border-bottom:1px solid var(--border)}.catalog-head h2{margin:2px 0 0;font-size:25px;letter-spacing:-.035em}.catalog-head p:not(.ff-kicker){margin:6px 0 0;color:var(--text-muted);font-size:11px}.catalog-search{height:46px;display:grid;grid-template-columns:22px 1fr;align-items:center;gap:7px;padding:0 12px;border:1px solid var(--border);border-radius:13px;background:var(--surface-2);color:var(--text-faint)}.catalog-search input{width:100%;border:0!important;outline:0!important;background:transparent!important;box-shadow:none!important;color:var(--text)}.conversion-composer{display:grid;grid-template-columns:minmax(170px,1.2fr) minmax(130px,.8fr) auto minmax(130px,.8fr) auto;align-items:end;gap:10px;margin:16px 0 4px;padding:15px;border:1px solid color-mix(in srgb,var(--accent) 25%,var(--border));border-radius:15px;background:linear-gradient(145deg,var(--accent-soft),var(--surface-2))}.conversion-composer>div{display:grid;grid-template-columns:40px 1fr;align-items:center;gap:9px}.composer-mark{width:38px;height:38px;display:grid;place-items:center;border-radius:11px;background:var(--accent);color:white}.conversion-composer strong,.conversion-composer small{display:block}.conversion-composer small{margin-top:3px;color:var(--text-muted);font-size:9px;line-height:1.35}.conversion-composer label{display:grid;gap:4px;color:var(--text-muted);font-size:8.5px;font-weight:800}.conversion-composer select{height:38px;padding:0 8px;border:1px solid var(--border);border-radius:9px;background:var(--surface-1);color:var(--text);font-size:10px}.composer-arrow{align-self:center;color:var(--accent);font-size:20px}.conversion-composer>button{height:38px;display:flex;align-items:center;gap:9px;padding:0 12px;border:0;border-radius:10px;background:var(--accent);color:white;font-size:10px;font-weight:850}.catalog-toolbar{display:flex;align-items:center;gap:6px;margin:14px 0}.catalog-toolbar button{min-height:31px;padding:0 10px;border:1px solid var(--border);border-radius:999px;background:var(--surface-2);color:var(--text-muted);font-size:9px;font-weight:800}.catalog-toolbar button.active{border-color:var(--accent);background:var(--accent-soft);color:var(--accent)}.catalog-toolbar>span{margin-left:auto;color:var(--text-faint);font-size:9px}.catalog-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:9px}.action-card{min-height:270px;display:flex;flex-direction:column;padding:15px;border:1px solid var(--border);border-radius:17px;background:var(--bg-elevated);transition:var(--transition)}.action-card:hover{transform:translateY(-2px);border-color:var(--border-strong);box-shadow:var(--shadow-sm)}.action-card.unavailable{opacity:.72}.action-top{display:flex;align-items:center;gap:8px}.action-mark{width:40px;height:40px;display:grid;place-items:center;border-radius:12px;background:var(--accent-soft);color:var(--accent);font-size:9px;font-weight:900}.kind-label{padding:4px 7px;border-radius:999px;background:var(--surface-2);color:var(--text-muted);font-size:8px;font-weight:850;text-transform:uppercase}.star{margin-left:auto;width:30px;height:30px;border:0;border-radius:9px;background:transparent;color:var(--warning);font-size:17px}.action-card h3{margin:14px 0 5px;font-size:15px}.action-card>p{min-height:34px;margin:0;color:var(--text-muted);font-size:10.5px;line-height:1.5}.action-meta{display:flex;flex-wrap:wrap;gap:5px;margin-top:11px}.action-meta span{max-width:100%;padding:5px 7px;border-radius:8px;background:var(--surface-2);overflow:hidden;color:var(--text-muted);font-size:8.5px;text-overflow:ellipsis;white-space:nowrap}.action-meta b{margin-right:4px;color:var(--text);font-size:8px}.missing{margin-top:8px;color:var(--warning);font-size:8.5px}.open-action{min-height:40px;display:flex;align-items:center;justify-content:space-between;margin-top:auto;padding:0 11px;border:0;border-radius:11px;background:linear-gradient(135deg,var(--accent),var(--violet));color:white;font-size:10px;font-weight:850}.open-action:disabled{background:var(--surface-3);color:var(--text-faint)}.catalog-empty{grid-column:1/-1;display:grid;justify-items:center;gap:5px;padding:54px;color:var(--text-muted)}.catalog-empty>span{font-size:28px}@media(max-width:1120px){.conversion-composer{grid-template-columns:1fr 1fr auto 1fr}.conversion-composer>div{grid-column:1/-1}}@media(max-width:1050px){.advanced-hero{grid-template-columns:1fr}.health-card{max-width:360px}.advanced-layout{grid-template-columns:1fr}.group-rail{position:static;display:flex;flex-wrap:wrap}.group-rail>.ff-kicker,.rail-separator{width:100%}.group-rail button{width:auto;min-width:150px;flex:1}}@media(max-width:760px){.catalog-grid,.catalog-head,.conversion-composer{grid-template-columns:1fr}.composer-arrow{display:none}.catalog{padding:14px}.catalog-toolbar>span{display:none}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AdvancedPage {
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly router = inject(Router);
  private readonly workspace = inject(WorkspaceStore);
  private readonly intents = inject(ConversionIntentStore);
  protected readonly search = signal('');
  protected readonly selectedGroup = signal<AdvancedGroup>('all');
  protected readonly availability = signal<'all' | 'ready' | 'favorites'>('all');
  protected readonly conversionSource = signal('png');
  protected readonly conversionTarget = signal('webp');
  protected readonly conversionSources = computed(() => this.capabilities.formats().filter((format) => format.convertTo.length > 0));
  protected readonly conversionTargets = computed(() => this.capabilities.formatCapability(this.conversionSource())?.convertTo ?? []);
  protected readonly groups: GroupItem[] = [
    { id: 'all', label: 'Vue d’ensemble', icon: '✦' }, { id: 'conversion', label: 'Conversion', icon: '↻' },
    { id: 'pdf', label: 'PDF & OCR', icon: 'PDF' }, { id: 'image', label: 'Images', icon: '◫' },
    { id: 'document', label: 'Documents', icon: 'T' }, { id: 'media', label: 'Audio & vidéo', icon: '▶' },
    { id: 'archive', label: 'Archives', icon: '▱' }, { id: 'organization', label: 'Organisation', icon: '⇄' },
    { id: 'privacy', label: 'Confidentialité', icon: '▣' },
  ];

  protected readonly filteredActions = computed(() => {
    const query = this.search().trim().toLowerCase();
    const group = this.selectedGroup();
    const availability = this.availability();
    return this.capabilities.actions().filter((action) => {
      if (!this.inGroup(action, group)) return false;
      if (availability === 'ready' && !this.capabilities.isActionExecutable(action)) return false;
      if (availability === 'favorites' && !this.capabilities.isFavorite(action.id)) return false;
      if (!query) return true;
      const spec = this.capabilities.uiSpec(action.id);
      return `${action.title} ${action.description} ${action.category} ${action.accepts.join(' ')} ${spec?.sourceFormats.join(' ') ?? ''} ${spec?.targetFormats.join(' ') ?? ''}`.toLowerCase().includes(query);
    });
  });

  protected groupCount(group: AdvancedGroup): number { return this.capabilities.actions().filter((action) => this.inGroup(action, group)).length; }
  protected groupTitle(): string { return this.groups.find((group) => group.id === this.selectedGroup())?.label ?? 'Toutes les actions'; }
  protected actionMark(action: ActionDescriptor): string { return action.title.slice(0, 2).toUpperCase(); }
  protected uiKind(action: ActionDescriptor): ActionUiKind { return this.capabilities.uiSpec(action.id)?.kind ?? 'generic'; }
  protected uiKindLabel(action: ActionDescriptor): string { return ({conversion:'Conversion',image:'Image',pdf:'PDF',media:'Média',archive:'Archive',extract:'Extraction',organization:'Organisation',privacy:'Confidentialité',generic:'Outil'} as Record<ActionUiKind,string>)[this.uiKind(action)]; }
  protected sourceSummary(action: ActionDescriptor): string { const formats = this.capabilities.uiSpec(action.id)?.sourceFormats ?? []; return formats.length ? summarize(formats) : 'Tout type compatible'; }
  protected targetSummary(action: ActionDescriptor): string { const formats = this.capabilities.uiSpec(action.id)?.targetFormats ?? []; return formats.length ? summarize(formats) : action.outputFormat?.toUpperCase() ?? ''; }
  protected parameterCount(action: ActionDescriptor): number { return this.capabilities.uiSpec(action.id)?.parameters.length ?? 0; }
  protected async toggleFavorite(action: ActionDescriptor): Promise<void> { try { await this.capabilities.toggleFavorite(action.id); } catch { /* rollback in store */ } }

  protected async startAction(action: ActionDescriptor): Promise<void> {
    const spec = this.capabilities.uiSpec(action.id);
    const parameters = Object.fromEntries((spec?.parameters ?? []).map((field) => [field.key, defaultParameterValue(field)]));
    this.intents.start({ actionId: action.id, sourceFormats: spec?.sourceFormats ?? [], targetFormat: spec?.defaultTarget ?? action.outputFormat ?? null, inputMode: spec?.inputMode ?? 'files', uiKind: spec?.kind ?? 'generic', parameters });
    this.workspace.startNewConversion(action.id);
    await this.router.navigate(['/conversion', action.id]);
  }

  protected setConversionSource(source: string): void {
    this.conversionSource.set(source);
    const targets = this.capabilities.formatCapability(source)?.convertTo ?? [];
    this.conversionTarget.set(targets[0] ?? '');
  }

  protected async launchFormatConversion(): Promise<void> {
    const source = this.conversionSource();
    const target = this.conversionTarget();
    const family = this.capabilities.formatCapability(source)?.family;
    const actionId = family === 'image' ? 'image-convert'
      : ['document','spreadsheet','presentation'].includes(family ?? '') ? 'office-convert'
      : family === 'audio' ? 'audio-convert'
      : family === 'video' ? (target === 'gif' ? 'video-to-gif' : 'video-convert')
      : family === 'pdf' ? (target === 'txt' ? 'pdf-extract-text' : target === 'pdf' ? 'smart-to-pdf' : 'pdf-to-images')
      : family === 'ebook' ? 'ebook-convert'
      : family === 'text' ? (target === 'pdf' ? 'text-to-pdf' : 'text-convert')
      : 'smart-to-pdf';
    const action = this.capabilities.action(actionId);
    if (!action || !this.capabilities.isActionExecutable(action)) return;
    const spec = this.capabilities.uiSpec(action.id);
    const parameters = Object.fromEntries((spec?.parameters ?? []).map((field) => [field.key, defaultParameterValue(field)]));
    this.intents.start({ actionId: action.id, sourceFormats: [source], strictSourceFormat: true, targetFormat: target, inputMode: 'files', uiKind: spec?.kind ?? 'conversion', parameters });
    this.workspace.startNewConversion(action.id);
    await this.router.navigate(['/conversion', action.id]);
  }

  private inGroup(action: ActionDescriptor, group: AdvancedGroup): boolean {
    if (group === 'all') return true;
    if (group === 'conversion') return action.category === 'convert';
    if (group === 'pdf') return action.category === 'pdf' || action.id.includes('ocr');
    if (group === 'image') return action.category === 'image' || (action.category === 'optimize' && action.accepts.includes('image'));
    if (group === 'document') return action.category === 'document' || action.accepts.some((family) => ['document','spreadsheet','presentation','ebook','text'].includes(family));
    if (group === 'media') return action.category === 'media';
    if (group === 'archive') return action.category === 'archive' || ['zstd-compress','zstd-decompress','lz4-compress','lz4-decompress'].includes(action.id);
    if (group === 'organization') return action.category === 'organize';
    return action.category === 'privacy';
  }
}

function summarize(values: string[]): string {
  const labels = values.slice(0, 4).map((value) => value.toUpperCase());
  return `${labels.join(', ')}${values.length > labels.length ? ` +${values.length - labels.length}` : ''}`;
}

function defaultParameterValue(field: ActionParameterDescriptor): string | number | boolean | null {
  const value = field.defaultValue ?? null;
  if (field.kind === 'toggle') return value === 'true';
  if (['number','range','time'].includes(field.kind)) { const numeric = Number(value); return Number.isFinite(numeric) ? numeric : null; }
  return value;
}
