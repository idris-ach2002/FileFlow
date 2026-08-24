import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';
import { ConversionIntentStore } from '../../core/conversion/conversion-intent.store';

@Component({
  selector: 'ff-favorites-page',
  template: `
    <div class="ff-page-narrow favorites-shell">
      <header class="page-head">
        <div><p class="ff-kicker">VOS RACCOURCIS</p><h1 class="ff-title">Favoris</h1><p class="ff-subtitle">Gardez ici les actions que vous utilisez souvent. Elles restent accessibles en un clic sans encombrer l’accueil.</p></div>
        <span class="count-orb">{{ capabilities.favoriteActions().length }}</span>
      </header>

      @if (capabilities.favoriteActions().length) {
        <section class="favorite-list ff-section">
          @for (action of capabilities.favoriteActions(); track action.id) {
            <article class="favorite-card">
              <button class="favorite-main" type="button" (click)="startAction(action)">
                <span class="ff-icon-badge">{{ action.title.slice(0,2).toUpperCase() }}</span>
                <span><strong>{{ action.title }}</strong><small>{{ action.description }}</small><em>{{ capabilities.isActionExecutable(action) ? 'Prêt sur cet appareil' : 'Indisponible actuellement' }}</em></span>
                <b>Commencer →</b>
              </button>
              <button class="remove" type="button" (click)="remove(action)">★ <span>Retirer</span></button>
            </article>
          }
        </section>
      } @else {
        <section class="empty ff-panel soft">
          <span class="star-orb">☆</span>
          <h2>Aucun favori pour le moment</h2>
          <p>Ajoutez une étoile à une action depuis l’accueil ou l’espace avancé. Vous la retrouverez ici.</p>
          <button class="ff-button" type="button" (click)="router.navigate(['/advanced'])">Explorer les actions</button>
        </section>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.page-head{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:20px;align-items:end}.count-orb{width:72px;height:72px;display:grid;place-items:center;border:1px solid color-mix(in srgb,var(--accent) 16%,var(--border));border-radius:22px;background:var(--accent-soft);color:var(--accent);font-size:28px;font-weight:900}.favorite-list{display:grid;gap:10px}.favorite-card{position:relative}.favorite-main{width:100%;min-height:112px;display:grid;grid-template-columns:52px minmax(0,1fr) auto;align-items:center;gap:14px;padding:17px 110px 17px 17px;border:1px solid var(--border);border-radius:19px;background:var(--surface-1);color:var(--text);text-align:left;box-shadow:var(--shadow-sm);transition:var(--transition)}.favorite-main:hover{transform:translateY(-2px);border-color:color-mix(in srgb,var(--accent) 24%,var(--border));box-shadow:var(--shadow-md)}.favorite-main strong,.favorite-main small,.favorite-main em{display:block}.favorite-main strong{font-size:16px}.favorite-main small{margin-top:5px;color:var(--text-muted);font-size:13px}.favorite-main em{margin-top:6px;color:var(--success);font-size:10px;font-style:normal;font-weight:800;text-transform:uppercase}.favorite-main>b{color:var(--accent);font-size:11px}.remove{position:absolute;right:13px;top:13px;display:flex;align-items:center;gap:5px;padding:6px 8px;border:0;border-radius:9px;background:var(--warning-soft);color:var(--warning);font-size:11px;font-weight:800}.empty{min-height:420px;display:grid;place-items:center;align-content:center;text-align:center;margin-top:34px}.star-orb{width:72px;height:72px;display:grid;place-items:center;border-radius:23px;background:var(--warning-soft);color:var(--warning);font-size:34px}.empty h2{margin:17px 0 0;font-size:26px}.empty p{max-width:520px;margin:8px 0 16px;color:var(--text-muted);font-size:14px;line-height:1.6}@media(max-width:620px){.page-head{grid-template-columns:1fr}.count-orb{width:58px;height:58px}.favorite-main{grid-template-columns:48px 1fr;padding-right:17px}.favorite-main>b{grid-column:2}.remove{position:static;margin-top:6px}.remove span{display:none}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FavoritesPage {
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly router = inject(Router);
  private readonly workspace = inject(WorkspaceStore);
  private readonly intents = inject(ConversionIntentStore);

  protected async remove(action: ActionDescriptor): Promise<void> { try { await this.capabilities.toggleFavorite(action.id); } catch { /* rollback in store */ } }
  protected async startAction(action: ActionDescriptor): Promise<void> {
    const spec = this.capabilities.uiSpec(action.id);
    this.intents.start({ actionId: action.id, sourceFormats: spec?.sourceFormats ?? [], targetFormat: spec?.defaultTarget ?? action.outputFormat ?? null, inputMode: spec?.inputMode ?? 'files', uiKind: spec?.kind ?? 'generic', parameters: {} });
    this.workspace.startNewConversion(action.id);
    await this.router.navigate(['/conversion', action.id]);
  }
}
