import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

interface ExpertArea {
  title: string;
  description: string;
  icon: string;
  tone: 'accent' | 'violet' | 'orange' | 'green' | 'blue' | 'neutral';
  route: string;
  badge?: string;
}

@Component({
  selector: 'ff-advanced-page',
  template: `
    <div class="ff-page advanced-shell">
      <header class="advanced-hero">
        <div>
          <div class="expert-badge"><span>✦</span> ESPACE EXPERT</div>
          <h1 class="ff-title">Toute la puissance de FileFlow, au même endroit.</h1>
          <p class="ff-subtitle">Cet espace est volontairement séparé de l’accueil. Vous pouvez explorer les formats, les moteurs, les automatisations et l’intégralité des actions sans compliquer l’expérience quotidienne.</p>
        </div>
        <div class="health-card">
          <span class="health-orb">{{ capabilities.engineReadyCount() }}</span>
          <div><strong>moteurs disponibles</strong><small>sur {{ capabilities.engines().length }} détectés</small></div>
          <span class="ff-badge success">● Local</span>
        </div>
      </header>

      <section class="expert-grid ff-section" aria-label="Espaces avancés">
        @for (area of areas; track area.title) {
          <button class="expert-card" [attr.data-tone]="area.tone" type="button" (click)="router.navigate([area.route])">
            <span class="area-icon">{{ area.icon }}</span>
            <span class="area-copy"><span class="area-top"><strong>{{ area.title }}</strong>@if(area.badge){<em>{{ area.badge }}</em>}</span><small>{{ area.description }}</small></span>
            <b>↗</b>
          </button>
        }
      </section>

      <section class="catalog ff-section ff-panel">
        <header class="catalog-head">
          <div><p class="ff-kicker">CATALOGUE COMPLET</p><h2 class="ff-section-title">Toutes les actions</h2><p>{{ capabilities.actions().length }} fonctions recensées · seules les fonctions exécutables sur cette machine peuvent être lancées.</p></div>
          <label class="catalog-search"><span>⌕</span><input #query type="search" placeholder="Rechercher OCR, PDF, HEIC, audio…" [value]="search()" (input)="search.set(query.value)" /></label>
        </header>

        <div class="catalog-toolbar">
          <span class="ff-badge accent">{{ filteredActions().length }} action(s)</span>
          <span class="ff-badge success">{{ readyActionCount() }} prête(s)</span>
          <span class="ff-badge">{{ capabilities.formats().length }} profils de format</span>
        </div>

        <div class="catalog-grid">
          @for (action of filteredActions(); track action.id) {
            <article class="catalog-action" [class.unavailable]="!capabilities.isActionExecutable(action)">
              <button class="catalog-main" type="button" [disabled]="capabilities.actionState(action) === 'planned'" (click)="startAction(action)">
                <span class="action-mark">{{ action.title.slice(0,2).toUpperCase() }}</span>
                <span><strong>{{ action.title }}</strong><small>{{ action.description }}</small><em>{{ categoryLabel(action.category) }}</em></span>
                <b>{{ capabilities.isActionExecutable(action) ? 'Ouvrir' : capabilities.actionState(action) === 'missing-engine' ? 'Moteur absent' : 'Planifié' }}</b>
              </button>
              <button class="star" type="button" (click)="toggleFavorite(action)" [attr.aria-label]="capabilities.isFavorite(action.id) ? 'Retirer des favoris' : 'Ajouter aux favoris'">{{ capabilities.isFavorite(action.id) ? '★' : '☆' }}</button>
            </article>
          } @empty {
            <div class="catalog-empty"><span>⌕</span><strong>Aucune action trouvée</strong><small>Essayez un terme plus général.</small></div>
          }
        </div>
      </section>

      <aside class="expert-note">
        <span>i</span><div><strong>Pourquoi cet espace est séparé&nbsp;?</strong><p>L’accueil reste simple pour les personnes qui ne connaissent pas les formats ou les outils. Ici, un utilisateur du domaine peut inspecter tout le catalogue et accéder aux contrôles avancés.</p></div>
      </aside>
    </div>
  `,
  styles: [`
    :host{display:block}.advanced-shell{padding-bottom:20px}.advanced-hero{display:grid;grid-template-columns:minmax(0,1fr) 280px;gap:28px;align-items:end}.expert-badge{display:inline-flex;align-items:center;gap:8px;margin-bottom:14px;padding:7px 10px;border:1px solid color-mix(in srgb,var(--accent) 14%,var(--border));border-radius:999px;background:var(--accent-soft);color:var(--accent-strong);font-size:10px;font-weight:900;letter-spacing:.1em}.expert-badge span{font-size:15px}.advanced-hero .ff-title{max-width:820px}.health-card{min-height:120px;display:grid;grid-template-columns:64px minmax(0,1fr);align-items:center;gap:12px;padding:18px;border:1px solid var(--border);border-radius:20px;background:linear-gradient(145deg,var(--surface-1),var(--accent-soft-2));box-shadow:var(--shadow-sm)}.health-orb{width:62px;height:62px;display:grid;place-items:center;grid-row:1/3;border-radius:20px;background:linear-gradient(145deg,var(--accent),var(--violet));color:white;font-size:24px;font-weight:900;box-shadow:0 12px 28px color-mix(in srgb,var(--accent) 22%,transparent)}.health-card strong,.health-card small{display:block}.health-card strong{font-size:14px}.health-card small{margin-top:3px;color:var(--text-muted);font-size:11.5px}.health-card .ff-badge{width:max-content}
    .expert-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:11px}.expert-card{min-height:140px;display:grid;grid-template-columns:52px minmax(0,1fr) auto;align-items:center;gap:13px;padding:18px;border:1px solid var(--border);border-radius:20px;background:var(--surface-1);color:var(--text);text-align:left;box-shadow:var(--shadow-sm);transition:var(--transition)}.expert-card:hover{transform:translateY(-3px);border-color:color-mix(in srgb,var(--accent) 25%,var(--border));box-shadow:var(--shadow-md)}.area-icon{width:50px;height:50px;display:grid;place-items:center;border-radius:16px;background:var(--accent-soft);color:var(--accent);font-size:19px;font-weight:900}.expert-card[data-tone='violet'] .area-icon{background:var(--accent-soft-2);color:var(--violet)}.expert-card[data-tone='orange'] .area-icon{background:var(--warning-soft);color:var(--warning)}.expert-card[data-tone='green'] .area-icon{background:var(--success-soft);color:var(--success)}.expert-card[data-tone='blue'] .area-icon{background:#eaf7fb;color:var(--cyan)}.expert-card[data-tone='neutral'] .area-icon{background:var(--surface-2);color:var(--text-muted)}.area-copy strong,.area-copy small{display:block}.area-copy strong{font-size:15px}.area-copy small{margin-top:6px;color:var(--text-muted);font-size:12.5px;line-height:1.48}.area-top{display:flex;align-items:center;gap:7px}.area-top em{padding:2px 6px;border-radius:999px;background:var(--accent-soft);color:var(--accent);font-size:8px;font-style:normal;font-weight:900}.expert-card>b{color:var(--text-faint);font-size:18px}
    .catalog{padding:23px}.catalog-head{display:grid;grid-template-columns:minmax(0,1fr) minmax(300px,410px);gap:20px;align-items:end;padding-bottom:18px;border-bottom:1px solid var(--border)}.catalog-head p:not(.ff-kicker){margin:6px 0 0;color:var(--text-muted);font-size:13px}.catalog-search{height:48px;display:grid;grid-template-columns:22px minmax(0,1fr);align-items:center;gap:8px;padding:0 12px;border:1px solid var(--border);border-radius:14px;background:var(--surface-2);color:var(--text-faint)}.catalog-search input{min-height:0!important;width:100%;border:0!important;background:transparent!important;box-shadow:none!important}.catalog-toolbar{display:flex;flex-wrap:wrap;gap:7px;margin:16px 0}.catalog-grid{display:grid;grid-template-columns:repeat(2,1fr);gap:8px}.catalog-action{position:relative}.catalog-main{width:100%;min-height:88px;display:grid;grid-template-columns:40px minmax(0,1fr) auto;align-items:center;gap:11px;padding:11px 42px 11px 11px;border:1px solid var(--border);border-radius:14px;background:var(--bg-elevated);color:var(--text);text-align:left;transition:var(--transition)}.catalog-main:hover:not(:disabled){background:var(--surface-2);border-color:var(--border-strong)}.catalog-action.unavailable .catalog-main{opacity:.66}.action-mark{width:38px;height:38px;display:grid;place-items:center;border-radius:12px;background:var(--accent-soft);color:var(--accent);font-size:9px;font-weight:900}.catalog-main strong,.catalog-main small,.catalog-main em{display:block}.catalog-main strong{font-size:13.5px}.catalog-main small{margin-top:3px;color:var(--text-muted);font-size:11.5px;line-height:1.35}.catalog-main em{margin-top:5px;color:var(--text-faint);font-size:9px;font-style:normal;text-transform:uppercase;font-weight:800}.catalog-main>b{color:var(--accent);font-size:10px;white-space:nowrap}.star{position:absolute;right:8px;top:8px;width:28px;height:28px;border:0;border-radius:8px;background:transparent;color:var(--warning);font-size:17px}.catalog-empty{grid-column:1/-1;display:grid;justify-items:center;gap:6px;padding:46px;color:var(--text-muted)}.catalog-empty>span{font-size:28px}.expert-note{display:flex;gap:12px;margin-top:20px;padding:16px 18px;border:1px solid color-mix(in srgb,var(--accent) 15%,var(--border));border-radius:16px;background:color-mix(in srgb,var(--accent-soft) 46%,var(--surface-1))}.expert-note>span{width:30px;height:30px;display:grid;place-items:center;flex:none;border-radius:10px;background:var(--accent);color:white;font-weight:900}.expert-note strong{font-size:13px}.expert-note p{margin:4px 0 0;color:var(--text-muted);font-size:12.5px;line-height:1.5}
    @media(max-width:1050px){.advanced-hero{grid-template-columns:1fr}.health-card{max-width:360px}.expert-grid{grid-template-columns:repeat(2,1fr)}}@media(max-width:760px){.expert-grid,.catalog-grid,.catalog-head{grid-template-columns:1fr}.catalog-search{width:100%}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AdvancedPage {
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly router = inject(Router);
  private readonly workspace = inject(WorkspaceStore);
  protected readonly search = signal('');
  protected readonly areas: ExpertArea[] = [
    {title:'Formats & possibilités',description:'Formats pris en charge, conversions, lecture, écriture et compatibilités réelles.',icon:'◇',tone:'accent',route:'/formats'},
    {title:'Organiser & nettoyer',description:'Renommage en lot, classement, doublons et opérations structurées.',icon:'⇄',tone:'green',route:'/organize'},
    {title:'Automatisations',description:'Règles, enchaînements et actions répétitives pour les usages avancés.',icon:'⚡',tone:'orange',route:'/automations'},
    {title:'Moteurs & diagnostic',description:'Voir les outils disponibles localement et diagnostiquer une fonctionnalité.',icon:'▣',tone:'blue',route:'/settings',badge:'LOCAL'},
    {title:'Aide technique',description:'Guides détaillés, comportements et explications des traitements.',icon:'?',tone:'violet',route:'/help'},
    {title:'Préférences expertes',description:'Performances, sécurité, destination, apparence et détails techniques.',icon:'⚙',tone:'neutral',route:'/settings'},
  ];
  protected readonly filteredActions = computed(() => {
    const query = this.search().trim().toLowerCase();
    const actions = this.capabilities.actions();
    if (!query) return actions;
    return actions.filter((action) => `${action.title} ${action.description} ${action.category} ${action.accepts.join(' ')} ${action.outputFormat ?? ''}`.toLowerCase().includes(query));
  });
  protected readonly readyActionCount = computed(() => this.capabilities.actions().filter((action) => this.capabilities.isActionExecutable(action)).length);

  protected categoryLabel(category: string): string { return category.replace(/[-_]/g,' '); }
  protected async toggleFavorite(action: ActionDescriptor): Promise<void> { try { await this.capabilities.toggleFavorite(action.id); } catch { /* rollback in store */ } }
  protected async startAction(action: ActionDescriptor): Promise<void> {
    this.workspace.setPendingAction(action.id);
    if (this.workspace.hasWorkspace()) { this.workspace.openAction(action.id); await this.router.navigate(['/workspace']); return; }
    const paths = DIRECTORY_FIRST_ACTIONS.has(action.id) ? await this.workspace.pickDirectories() : await this.workspace.pickFiles();
    if (!paths.length) return;
    await this.router.navigate(['/workspace']);
    await this.workspace.start(paths);
  }
}

const DIRECTORY_FIRST_ACTIONS = new Set(['tar-zstd-create','tar-lz4-create','archive-create']);
