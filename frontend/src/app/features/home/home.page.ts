import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

interface IntentCard {
  id: string;
  title: string;
  description: string;
  icon: string;
  tone: 'pdf' | 'image' | 'media' | 'archive';
  actionIds: string[];
}

const INTENTS: IntentCard[] = [
  { id:'pdf', title:'PDF & documents', description:'Convertir, fusionner, compresser ou lire un scan.', icon:'PDF', tone:'pdf', actionIds:['pdf-compress','pdf-merge','pdf-split','office-to-pdf','pdf-ocr','pdf-extract-text','images-to-pdf','text-convert','ebook-convert'] },
  { id:'images', title:'Photos & images', description:'Convertir, réduire, redimensionner ou extraire du texte.', icon:'IMG', tone:'image', actionIds:['image-convert','image-batch-convert','image-optimize','image-resize','ocr-image','strip-metadata'] },
  { id:'media', title:'Audio & vidéo', description:'Convertir, compresser, extraire l’audio ou créer un GIF.', icon:'▶', tone:'media', actionIds:['media-compatible','video-convert','media-compress','audio-convert','extract-audio','video-to-gif'] },
  { id:'archives', title:'Archives', description:'Ouvrir, extraire ou créer ZIP, 7Z, TAR et plus.', icon:'ZIP', tone:'archive', actionIds:['archive-extract','zstd-decompress','lz4-decompress','tar-zstd-create','tar-lz4-create','archive-create'] },
];

@Component({
  selector: 'ff-home-page',
  template: `
    <div class="ff-page home-shell">
      <header class="hero">
        <div class="hero-copy">
          <div class="hello"><span class="hello-dot"></span> Bonjour {{ auth.profile()?.firstName || auth.profile()?.displayName }}</div>
          <h1 class="ff-display">Que voulez-vous faire aujourd’hui&nbsp;?</h1>
          <p class="ff-subtitle">Commencez par votre fichier. FileFlow le reconnaît, masque la technique et vous propose uniquement les actions qui ont du sens.</p>
        </div>
        <button class="help-card" type="button" (click)="router.navigate(['/help'])">
          <span class="ff-icon-badge">?</span>
          <span><strong>Besoin d’aide&nbsp;?</strong><small>Dites simplement ce que vous cherchez à obtenir.</small></span>
          <b>›</b>
        </button>
      </header>

      <section class="drop-zone" [class.busy]="store.busy()" aria-label="Ajouter des fichiers">
        <div class="drop-glow"></div>
        @if (store.busy()) {
          <span class="upload-orb pulse">…</span>
          <h2>Je regarde vos fichiers…</h2>
          <p>{{ store.stats().discovered }} élément(s) détecté(s) · {{ formatBytes(store.stats().totalBytes) }}</p>
        } @else {
          <span class="upload-orb">⇧</span>
          <h2>Glissez-déposez vos fichiers ici</h2>
          <p>PDF, photos, documents, vidéos, archives… FileFlow s’occupe du reste.</p>
          <div class="drop-actions">
            <button class="ff-button" type="button" (click)="chooseFiles()"><span>▰</span> Choisir un fichier</button>
            <button class="ff-button secondary" type="button" (click)="chooseDirectories()">Choisir un dossier</button>
          </div>
        }
        <div class="local-note"><span>◆</span> Traitement local · vos fichiers ne quittent pas cet appareil</div>
      </section>

      <section class="ff-section quick-section">
        <div class="ff-section-head">
          <div><p class="ff-kicker">RACCOURCIS</p><h2 class="ff-section-title">Ou choisissez une famille</h2><p>Quatre points d’entrée simples. Les fonctions expertes restent dans l’espace avancé.</p></div>
        </div>
        <div class="intent-grid">
          @for (intent of intents; track intent.id) {
            <button class="intent-card" [attr.data-tone]="intent.tone" type="button" [class.active]="selectedIntent() === intent.id" (click)="selectedIntent.set(selectedIntent() === intent.id ? null : intent.id)">
              <span class="intent-icon">{{ intent.icon }}</span>
              <span><strong>{{ intent.title }}</strong><small>{{ intent.description }}</small></span>
              <b>›</b>
            </button>
          }
        </div>
      </section>

      @if (selectedIntent(); as selected) {
        <section class="action-picker ff-panel">
          <header>
            <div><p class="ff-kicker">{{ intentTitle(selected) }}</p><h2 class="ff-section-title">Que souhaitez-vous obtenir&nbsp;?</h2><p>Choisissez une action. Les réglages apparaîtront seulement après votre choix.</p></div>
            <button class="close-picker" type="button" (click)="selectedIntent.set(null)">Fermer ×</button>
          </header>
          <div class="simple-actions">
            @for (action of intentActions(); track action.id) {
              <article>
                <button class="simple-action" type="button" [disabled]="capabilities.actionState(action) === 'planned'" (click)="startAction(action)">
                  <span class="action-status" [class.ready]="capabilities.actionState(action) === 'ready'">{{ capabilities.actionState(action) === 'ready' ? '✓' : '…' }}</span>
                  <span><strong>{{ action.title }}</strong><small>{{ action.description }}</small></span>
                  <b>{{ capabilities.actionState(action) === 'ready' ? 'Commencer' : capabilities.actionState(action) === 'missing-engine' ? 'À installer' : 'Bientôt' }}</b>
                </button>
                <button class="favorite" type="button" [attr.aria-label]="capabilities.isFavorite(action.id) ? 'Retirer des favoris' : 'Ajouter aux favoris'" (click)="toggleFavorite(action)">{{ capabilities.isFavorite(action.id) ? '★' : '☆' }}</button>
              </article>
            } @empty { <p class="no-action">Aucune action de cette catégorie n’est encore disponible.</p> }
          </div>
        </section>
      }

      @if (capabilities.favoriteActions().length) {
        <section class="ff-section favorites-preview">
          <div class="ff-section-head">
            <div><p class="ff-kicker">VOS HABITUDES</p><h2 class="ff-section-title">Reprendre en un clic</h2></div>
            <button class="text-link" type="button" (click)="router.navigate(['/favorites'])">Tous les favoris →</button>
          </div>
          <div class="favorite-grid">
            @for (action of capabilities.favoriteActions().slice(0, 4); track action.id) {
              <button class="favorite-action" type="button" (click)="startAction(action)"><span>★</span><div><strong>{{ action.title }}</strong><small>{{ action.description }}</small></div><b>›</b></button>
            }
          </div>
        </section>
      }

      <footer class="home-footer">
        <div><span>✓</span><strong>Originaux conservés</strong></div>
        <div><span>◆</span><strong>100 % local</strong></div>
        <div><span>✦</span><strong>Guidé par défaut</strong></div>
        <button type="button" (click)="router.navigate(['/advanced'])">Voir tout ce que FileFlow sait faire <b>→</b></button>
      </footer>
    </div>
  `,
  styles: [`
    :host{display:block}.home-shell{padding-bottom:18px}.hero{display:grid;grid-template-columns:minmax(0,1fr) 290px;gap:34px;align-items:end}.hello{display:flex;align-items:center;gap:8px;margin-bottom:16px;color:var(--text-muted);font-size:13px;font-weight:800;text-transform:uppercase;letter-spacing:.06em}.hello-dot{width:8px;height:8px;border-radius:50%;background:var(--success);box-shadow:0 0 0 5px color-mix(in srgb,var(--success) 11%,transparent)}.hero .ff-display{max-width:860px}.help-card{min-height:94px;display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:13px;padding:16px;border:1px solid var(--border);border-radius:19px;background:var(--surface-1);color:var(--text);text-align:left;box-shadow:var(--shadow-sm);transition:var(--transition)}.help-card:hover{transform:translateY(-2px);border-color:color-mix(in srgb,var(--accent) 25%,var(--border));box-shadow:var(--shadow-md)}.help-card strong,.help-card small{display:block}.help-card strong{font-size:15px}.help-card small{margin-top:4px;color:var(--text-muted);font-size:12.5px;line-height:1.45}.help-card>b{color:var(--text-faint);font-size:24px}
    .drop-zone{position:relative;isolation:isolate;min-height:310px;display:grid;place-items:center;align-content:center;gap:8px;margin-top:36px;padding:34px;border:1.5px dashed color-mix(in srgb,var(--accent) 28%,var(--border-strong));border-radius:28px;background:linear-gradient(145deg,color-mix(in srgb,var(--surface-1) 97%,transparent),color-mix(in srgb,var(--accent-soft) 42%,var(--surface-1)));overflow:hidden;text-align:center;box-shadow:var(--shadow-sm);transition:var(--transition)}.drop-zone:hover{border-color:color-mix(in srgb,var(--accent) 60%,var(--border));box-shadow:var(--shadow-md)}.drop-glow{position:absolute;z-index:-1;width:420px;height:220px;top:-150px;border-radius:50%;background:color-mix(in srgb,var(--accent) 16%,transparent);filter:blur(50px)}.upload-orb{width:68px;height:68px;display:grid;place-items:center;margin-bottom:5px;border-radius:22px;background:linear-gradient(145deg,var(--accent-soft),var(--accent-soft-2));color:var(--accent);box-shadow:inset 0 0 0 1px color-mix(in srgb,var(--accent) 8%,transparent);font-size:29px;font-weight:700}.drop-zone h2{margin:6px 0 0;font-size:26px;letter-spacing:-.04em}.drop-zone>p{max-width:570px;margin:2px 0 0;color:var(--text-muted);font-size:14px}.drop-actions{display:flex;gap:10px;margin-top:14px}.local-note{margin-top:15px;color:var(--text-faint);font-size:11.5px;font-weight:680}.local-note span{margin-right:5px;color:var(--success)}.pulse{animation:pulse 1s infinite alternate}@keyframes pulse{to{opacity:.45;transform:scale(.97)}}
    .intent-grid{display:grid;grid-template-columns:repeat(4,1fr);gap:11px}.intent-card{min-height:142px;display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:12px;padding:17px;border:1px solid var(--border);border-radius:19px;background:var(--surface-1);color:var(--text);text-align:left;box-shadow:var(--shadow-sm);transition:var(--transition)}.intent-card:hover,.intent-card.active{transform:translateY(-2px);border-color:color-mix(in srgb,var(--accent) 28%,var(--border));box-shadow:var(--shadow-md)}.intent-icon{width:46px;height:46px;display:grid;place-items:center;border-radius:14px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:900}.intent-card[data-tone='pdf'] .intent-icon{background:var(--danger-soft);color:var(--danger)}.intent-card[data-tone='image'] .intent-icon{background:var(--success-soft);color:var(--success)}.intent-card[data-tone='media'] .intent-icon{background:var(--accent-soft-2);color:var(--violet)}.intent-card[data-tone='archive'] .intent-icon{background:var(--warning-soft);color:var(--warning)}.intent-card strong,.intent-card small{display:block}.intent-card strong{font-size:15px}.intent-card small{margin-top:6px;color:var(--text-muted);font-size:12.5px;line-height:1.45}.intent-card>b{color:var(--text-faint);font-size:23px}
    .action-picker{margin-top:15px;padding:22px}.action-picker>header{display:flex;justify-content:space-between;align-items:start;gap:15px;padding-bottom:17px;border-bottom:1px solid var(--border)}.action-picker>header p:not(.ff-kicker){margin:5px 0 0;color:var(--text-muted);font-size:13px}.close-picker,.text-link{border:0;background:transparent;color:var(--accent);font-weight:800}.simple-actions{display:grid;grid-template-columns:repeat(2,1fr);gap:9px;margin-top:15px}.simple-actions article{position:relative}.simple-action{width:100%;min-height:88px;display:grid;grid-template-columns:38px minmax(0,1fr) auto;align-items:center;gap:11px;padding:12px 45px 12px 12px;border:1px solid transparent;border-radius:14px;background:var(--bg-elevated);color:var(--text);text-align:left;transition:var(--transition)}.simple-action:hover:not(:disabled){border-color:var(--border-strong);background:var(--surface-2)}.simple-action:disabled{opacity:.54;cursor:default}.action-status{width:34px;height:34px;display:grid;place-items:center;border-radius:11px;background:var(--warning-soft);color:var(--warning);font-size:11px;font-weight:900}.action-status.ready{background:var(--success-soft);color:var(--success)}.simple-action strong,.simple-action small{display:block}.simple-action strong{font-size:14px}.simple-action small{margin-top:4px;color:var(--text-muted);font-size:12px;line-height:1.45}.simple-action>b{color:var(--accent);font-size:11px;white-space:nowrap}.favorite{position:absolute;right:9px;top:9px;width:30px;height:30px;border:0;border-radius:9px;background:transparent;color:var(--warning);font-size:17px}
    .favorite-grid{display:grid;grid-template-columns:repeat(2,1fr);gap:9px}.favorite-action{min-height:76px;display:grid;grid-template-columns:34px 1fr auto;align-items:center;gap:10px;padding:12px;border:1px solid var(--border);border-radius:15px;background:var(--surface-1);color:var(--text);text-align:left;transition:var(--transition)}.favorite-action:hover{background:var(--surface-2);border-color:var(--border-strong)}.favorite-action>span{color:var(--warning);font-size:17px}.favorite-action strong,.favorite-action small{display:block}.favorite-action strong{font-size:13px}.favorite-action small{margin-top:3px;color:var(--text-muted);font-size:11.5px}.favorite-action>b{color:var(--text-faint)}
    .home-footer{display:flex;align-items:center;gap:18px;margin-top:34px;padding:17px 4px;border-top:1px solid var(--border);color:var(--text-muted)}.home-footer>div{display:flex;align-items:center;gap:7px;font-size:11.5px}.home-footer>div span{color:var(--success)}.home-footer>button{margin-left:auto;border:0;background:transparent;color:var(--accent);font-size:12px;font-weight:800}.home-footer>button b{margin-left:5px}.no-action{grid-column:1/-1;color:var(--text-muted)}
    @media(max-width:1050px){.hero{grid-template-columns:1fr}.help-card{display:none}.intent-grid{grid-template-columns:repeat(2,1fr)}}@media(max-width:680px){.intent-grid,.simple-actions,.favorite-grid{grid-template-columns:1fr}.drop-zone{min-height:280px;padding:25px 18px}.drop-actions{flex-direction:column;width:100%;max-width:300px}.home-footer{align-items:flex-start;flex-direction:column}.home-footer>button{margin-left:0}.hero .ff-subtitle{font-size:15px}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  protected readonly store = inject(WorkspaceStore);
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly auth = inject(AuthStore);
  protected readonly router = inject(Router);
  protected readonly intents = INTENTS;
  protected readonly selectedIntent = signal<string | null>(null);
  protected readonly intentActions = computed(() => {
    const selected = INTENTS.find((intent) => intent.id === this.selectedIntent());
    if (!selected) return [];
    const byId = new Map(this.capabilities.actions().map((action) => [action.id, action]));
    return selected.actionIds.map((id) => byId.get(id)).filter((action): action is ActionDescriptor => Boolean(action));
  });

  protected intentTitle(id: string): string { return INTENTS.find((intent) => intent.id === id)?.title ?? 'Actions'; }
  protected async chooseFiles(): Promise<void> { await this.begin(await this.store.pickFiles()); }
  protected async chooseDirectories(): Promise<void> { await this.begin(await this.store.pickDirectories()); }
  protected async startAction(action: ActionDescriptor): Promise<void> {
    this.store.setPendingAction(action.id);
    if (this.store.hasWorkspace()) { this.store.openAction(action.id); await this.router.navigate(['/workspace']); return; }
    const paths = DIRECTORY_FIRST_ACTIONS.has(action.id) ? await this.store.pickDirectories() : await this.store.pickFiles();
    await this.begin(paths);
  }
  protected async toggleFavorite(action: ActionDescriptor): Promise<void> { try { await this.capabilities.toggleFavorite(action.id); } catch { /* store rolls back */ } }
  protected formatBytes(bytes: number): string { if(bytes<1024)return`${bytes} o`;const units=['Ko','Mo','Go','To'];let value=bytes/1024,index=0;while(index<units.length-1&&value>=1024){value/=1024;index++}return`${value>=10?value.toFixed(0):value.toFixed(1)} ${units[index]}`; }
  private async begin(paths: string[]): Promise<void> { if(!paths.length)return;await this.router.navigate(['/workspace']);await this.store.start(paths); }
}

const DIRECTORY_FIRST_ACTIONS = new Set(['tar-zstd-create','tar-lz4-create','archive-create']);
