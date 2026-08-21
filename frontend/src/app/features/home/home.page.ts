import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor } from '../../core/ipc/tauri.models';
import { PreferencesService } from '../../core/preferences/preferences.service';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

interface IntentCard {
  id: string;
  title: string;
  description: string;
  icon: string;
  actionIds: string[];
}

const INTENTS: IntentCard[] = [
  { id:'pdf', title:'PDF & documents', description:'Réduire, fusionner, convertir, lire un scan.', icon:'PDF', actionIds:['pdf-compress','pdf-merge','pdf-split','office-to-pdf','pdf-ocr','pdf-extract-text','images-to-pdf','text-convert','ebook-convert'] },
  { id:'images', title:'Photos & images', description:'JPG, PNG, HEIC, WebP, RAW et lots.', icon:'IMG', actionIds:['image-convert','image-batch-convert','image-optimize','image-resize','ocr-image','strip-metadata'] },
  { id:'compress', title:'Compresser', description:'Gagner de la place ou créer une archive.', icon:'↓', actionIds:['tar-zstd-create','tar-lz4-create','zstd-compress','lz4-compress','archive-create','pdf-compress','image-optimize','media-compress'] },
  { id:'archives', title:'Ouvrir & extraire', description:'ZIP, 7Z, RAR, TAR, Zstandard et plus.', icon:'ZIP', actionIds:['archive-extract','zstd-decompress','lz4-decompress','pdf-extract-text','extract-audio','extract-metadata'] },
  { id:'media', title:'Audio & vidéo', description:'Compatibilité, compression, audio et GIF.', icon:'▶', actionIds:['media-compatible','video-convert','media-compress','audio-convert','extract-audio','video-to-gif'] },
  { id:'organize', title:'Ranger & nettoyer', description:'Doublons, renommage, classement et vie privée.', icon:'✓', actionIds:['duplicate-scan','batch-rename','organize-by-type','strip-metadata','extract-metadata'] },
];

@Component({
  selector: 'ff-home-page',
  template: `
    <div class="home-shell">
      <header class="welcome-head">
        <div><p class="ff-kicker">BONJOUR {{ auth.profile()?.firstName || auth.profile()?.displayName }}</p><h1>Qu’est-ce que vous voulez faire ?</h1><p>Choisissez un résultat. FileFlow s’occupe des formats et des outils derrière.</p></div>
        <button class="help-button" type="button" (click)="router.navigate(['/help'])"><span>?</span><strong>Besoin d’aide ?</strong><small>Expliquez votre besoin avec vos mots</small></button>
      </header>

      <section class="intent-grid" aria-label="Choisir ce que je veux faire">
        @for (intent of intents; track intent.id) {
          <button class="intent-card" type="button" [class.active]="selectedIntent() === intent.id" (click)="selectedIntent.set(selectedIntent() === intent.id ? null : intent.id)">
            <span class="intent-icon">{{ intent.icon }}</span><span><strong>{{ intent.title }}</strong><small>{{ intent.description }}</small></span><b>›</b>
          </button>
        }
      </section>

      @if (selectedIntent(); as selected) {
        <section class="action-picker ff-card">
          <header><div><p class="ff-kicker">{{ intentTitle(selected) }}</p><h2>Choisissez simplement l’action</h2></div><button type="button" (click)="selectedIntent.set(null)">Fermer</button></header>
          <div class="simple-actions">
            @for (action of intentActions(); track action.id) {
              <article>
                <button class="simple-action" type="button" [disabled]="capabilities.actionState(action) === 'planned'" (click)="startAction(action)">
                  <span class="action-status" [class.ready]="capabilities.actionState(action) === 'ready'">{{ capabilities.actionState(action) === 'ready' ? '✓' : '…' }}</span>
                  <span><strong>{{ action.title }}</strong><small>{{ action.description }}</small></span>
                  <b>{{ capabilities.actionState(action) === 'ready' ? 'Commencer' : capabilities.actionState(action) === 'missing-engine' ? 'Moteur à installer' : 'Bientôt' }}</b>
                </button>
                <button class="favorite" type="button" (click)="toggleFavorite(action)">{{ capabilities.isFavorite(action.id) ? '★' : '☆' }}</button>
              </article>
            } @empty { <p class="no-action">Aucune action de cette catégorie n’est encore chargée.</p> }
          </div>
        </section>
      }

      <section class="add-section">
        <div class="add-copy"><p class="ff-kicker">OU COMMENCEZ PAR VOS FICHIERS</p><h2>Ajoutez ce que vous avez.</h2><p>FileFlow reconnaît automatiquement le contenu et vous proposera ensuite les actions possibles.</p></div>
        <div class="drop-zone" [class.busy]="store.busy()">
          @if (store.busy()) {
            <span class="big-plus pulse">…</span><strong>Analyse en cours</strong><small>{{ store.stats().discovered }} éléments · {{ formatBytes(store.stats().totalBytes) }}</small>
          } @else {
            <span class="big-plus">＋</span><strong>Glissez ici depuis Finder</strong><small>ou choisissez avec un bouton</small>
            <div><button class="ff-button" type="button" (click)="chooseFiles()">Fichiers</button><button class="ff-button secondary" type="button" (click)="chooseDirectories()">Dossier</button></div>
          }
        </div>
      </section>

      <section class="reassurance">
        <article><span>✓</span><div><strong>Originaux conservés</strong><small>Les résultats sont de nouveaux fichiers.</small></div></article>
        <article><span>⌂</span><div><strong>Traitement local</strong><small>Vos documents restent sur cet appareil.</small></div></article>
        <article><span>?</span><div><strong>Aide partout</strong><small>Chaque action possède un mode d’emploi.</small></div></article>
        <article><span>⚡</span><div><strong>Ordinateur réactif</strong><small>Les tâches lourdes sont limitées automatiquement.</small></div></article>
      </section>

      @if (!prefs.beginnerMode() && capabilities.health(); as health) {
        <details class="technical-state"><summary>État technique FileFlow</summary><div><span>{{ capabilities.engineReadyCount() }}/{{ capabilities.engines().length }} moteurs</span><span>{{ health.cpuThreads }} threads CPU</span><span>{{ health.scheduler.budget.memoryMb }} Mo RAM réservables</span><span>{{ health.os }} · {{ health.architecture }}</span></div></details>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.home-shell{max-width:1180px;margin:0 auto}.welcome-head{display:grid;grid-template-columns:minmax(0,1fr) 260px;gap:26px;align-items:end}.welcome-head h1{max-width:760px;margin:0;font-size:clamp(42px,6vw,68px);line-height:.98;letter-spacing:-.06em}.welcome-head>div>p:last-child{margin:16px 0 0;color:var(--text-muted);font-size:15px;line-height:1.6}.help-button{min-height:76px;display:grid;grid-template-columns:38px 1fr;grid-template-rows:auto auto;gap:2px 10px;align-content:center;padding:12px;border:1px solid var(--border);border-radius:15px;background:var(--surface-1);color:var(--text);text-align:left}.help-button:hover{border-color:var(--accent)}.help-button>span{grid-row:1/3;width:36px;height:36px;display:grid;place-items:center;border-radius:11px;background:var(--accent-soft);color:var(--accent);font-weight:900}.help-button strong{font-size:12px}.help-button small{color:var(--text-muted);font-size:11px;line-height:1.4}.intent-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-top:34px}.intent-card{min-height:112px;display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:12px;padding:16px;border:1px solid var(--border);border-radius:16px;background:var(--surface-1);color:var(--text);text-align:left;box-shadow:var(--shadow-sm);transition:transform var(--transition),border-color var(--transition),background var(--transition)}.intent-card:hover{transform:translateY(-2px);border-color:color-mix(in srgb,var(--accent) 30%,var(--border))}.intent-card.active{border-color:var(--accent);background:color-mix(in srgb,var(--accent-soft) 38%,var(--surface-1))}.intent-icon{width:46px;height:46px;display:grid;place-items:center;border-radius:14px;background:var(--surface-2);color:var(--accent);font-size:11px;font-weight:900}.intent-card strong,.intent-card small{display:block}.intent-card strong{font-size:14px}.intent-card small{margin-top:5px;color:var(--text-muted);font-size:11px;line-height:1.45}.intent-card>b{color:var(--text-faint);font-size:22px}.action-picker{margin-top:12px;padding:17px}.action-picker>header{display:flex;justify-content:space-between;align-items:start;gap:12px;padding-bottom:13px;border-bottom:1px solid var(--border)}.action-picker h2{margin:0;font-size:20px}.action-picker>header>button{border:0;background:transparent;color:var(--text-muted);font-size:11px}.simple-actions{display:grid;grid-template-columns:repeat(2,1fr);gap:7px;margin-top:12px}.simple-actions article{position:relative}.simple-action{width:100%;min-height:76px;display:grid;grid-template-columns:32px minmax(0,1fr) auto;align-items:center;gap:9px;padding:10px 42px 10px 10px;border:1px solid transparent;border-radius:11px;background:var(--bg-elevated);color:var(--text);text-align:left}.simple-action:hover:not(:disabled){border-color:var(--border-strong);background:var(--surface-2)}.simple-action:disabled{opacity:.55;cursor:default}.action-status{width:28px;height:28px;display:grid;place-items:center;border-radius:9px;background:var(--warning-soft);color:var(--warning);font-size:10px;font-weight:900}.action-status.ready{background:var(--success-soft);color:var(--success)}.simple-action strong,.simple-action small{display:block}.simple-action strong{font-size:12px}.simple-action small{margin-top:3px;color:var(--text-muted);font-size:10.5px;line-height:1.45}.simple-action>b{color:var(--accent);font-size:10.5px;white-space:nowrap}.favorite{position:absolute;right:8px;top:8px;width:28px;height:28px;border:0;border-radius:8px;background:transparent;color:var(--warning);font-size:16px}.add-section{margin-top:36px;display:grid;grid-template-columns:330px minmax(0,1fr);gap:20px;align-items:stretch}.add-copy{padding:18px 0}.add-copy h2{margin:0;font-size:27px;letter-spacing:-.04em}.add-copy>p:last-child{color:var(--text-muted);font-size:12px;line-height:1.6}.drop-zone{min-height:210px;display:grid;place-items:center;align-content:center;gap:6px;padding:22px;border:1.5px dashed var(--border-strong);border-radius:20px;background:linear-gradient(180deg,var(--surface-1),color-mix(in srgb,var(--accent-soft) 24%,var(--surface-1)));text-align:center}.drop-zone:hover{border-color:var(--accent)}.big-plus{width:48px;height:48px;display:grid;place-items:center;border-radius:15px;background:var(--accent-soft);color:var(--accent);font-size:26px}.drop-zone strong{font-size:13px}.drop-zone small{color:var(--text-muted);font-size:11px}.drop-zone>div{display:flex;gap:8px;margin-top:8px}.pulse{animation:pulse 1s infinite alternate}@keyframes pulse{to{opacity:.45}}.reassurance{display:grid;grid-template-columns:repeat(4,1fr);gap:8px;margin-top:28px}.reassurance article{display:grid;grid-template-columns:28px 1fr;gap:8px;align-items:start;padding:10px}.reassurance article>span{width:27px;height:27px;display:grid;place-items:center;border-radius:8px;background:var(--surface-2);color:var(--success);font-size:10px;font-weight:900}.reassurance strong,.reassurance small{display:block}.reassurance strong{font-size:11px}.reassurance small{margin-top:3px;color:var(--text-faint);font-size:10.5px;line-height:1.45}.technical-state{margin-top:25px;border-top:1px solid var(--border);padding-top:13px;color:var(--text-faint);font-size:10px}.technical-state summary{cursor:pointer}.technical-state div{display:flex;flex-wrap:wrap;gap:14px;margin-top:9px}@media(max-width:960px){.welcome-head{grid-template-columns:1fr}.help-button{display:none}.intent-grid{grid-template-columns:repeat(2,1fr)}.add-section{grid-template-columns:1fr}.reassurance{grid-template-columns:repeat(2,1fr)}}@media(max-width:620px){.welcome-head h1{font-size:42px}.intent-grid,.simple-actions,.reassurance{grid-template-columns:1fr}.intent-card{min-height:92px}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  protected readonly store = inject(WorkspaceStore);
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly auth = inject(AuthStore);
  protected readonly prefs = inject(PreferencesService);
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
    const paths = DIRECTORY_FIRST_ACTIONS.has(action.id)
      ? await this.store.pickDirectories()
      : await this.store.pickFiles();
    await this.begin(paths);
  }
  protected async toggleFavorite(action: ActionDescriptor): Promise<void> { try { await this.capabilities.toggleFavorite(action.id); } catch { /* store rolls back */ } }
  protected formatBytes(bytes: number): string { if(bytes<1024)return`${bytes} o`;const units=['Ko','Mo','Go','To'];let value=bytes/1024,index=0;while(index<units.length-1&&value>=1024){value/=1024;index++}return`${value>=10?value.toFixed(0):value.toFixed(1)} ${units[index]}`; }
  private async begin(paths: string[]): Promise<void> { if(!paths.length)return;await this.router.navigate(['/workspace']);await this.store.start(paths); }
}

const DIRECTORY_FIRST_ACTIONS = new Set(['tar-zstd-create','tar-lz4-create','archive-create']);
