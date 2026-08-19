import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor, OperationCategory } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

@Component({
  selector: 'ff-home-page',
  template: `
    <div class="home-shell">
      <header class="hero">
        <div class="hero-copy">
          <p class="ff-kicker">FILEFLOW · LOCAL FIRST</p>
          <h1>Vos fichiers, <span>sans friction.</span></h1>
          <p class="hero-lead">Déposez ce que vous avez. FileFlow identifie les formats, comprend le contexte et prépare les actions pertinentes sans envoyer vos documents sur Internet.</p>
        </div>
        <div class="hero-trust ff-card">
          <div class="trust-orb"><span>●</span></div>
          <div><strong>Traitement 100 % local</strong><span>PDF, photos, archives, documents et médias restent sur cet appareil.</span></div>
        </div>
      </header>

      <section class="drop-zone" [class.busy]="store.busy()" aria-label="Zone de dépôt de fichiers">
        <div class="drop-glow"></div>
        <div class="drop-symbol">＋</div>
        @if (store.busy()) {
          <div class="drop-copy"><strong>Analyse en cours…</strong><span>{{ store.stats().discovered }} éléments · {{ formatBytes(store.stats().totalBytes) }}</span></div>
          <div class="scan-line"><span></span></div>
        } @else {
          <div class="drop-copy"><strong>Déposez fichiers, dossiers ou archives</strong><span>FileFlow organise automatiquement la suite.</span></div>
          <div class="drop-actions">
            <button class="ff-button" type="button" (click)="chooseFiles()">Choisir des fichiers</button>
            <button class="ff-button secondary" type="button" (click)="chooseDirectories()">Choisir un dossier</button>
          </div>
          <div class="format-hint"><span>PDF</span><span>HEIC</span><span>DOCX</span><span>ZIP</span><span>MP4</span><span>EPUB</span><span>+ beaucoup d’autres</span></div>
        }
      </section>

      <section class="section-head">
        <div><p class="ff-kicker">ACTIONS INTELLIGENTES</p><h2>Les tâches du quotidien, en un clic</h2></div>
        <span>{{ capabilities.actions().length || 30 }} capacités prêtes dans le moteur</span>
      </section>

      <section class="action-grid" aria-label="Actions rapides">
        @for (action of featuredActions(); track action.id) {
          <button class="action-card ff-card" type="button" (click)="startAction(action)">
            <span class="action-mark" [attr.data-category]="action.category">{{ actionMark(action.category) }}</span>
            <span class="action-content"><strong>{{ action.title }}</strong><small>{{ action.description }}</small></span>
            <span class="action-state" [class.missing]="!capabilities.isActionReady(action)">
              {{ capabilities.isActionReady(action) ? 'Prêt' : 'À installer' }}
            </span>
          </button>
        }
      </section>

      <section class="feature-grid">
        <article class="feature-card feature-large ff-card">
          <div class="feature-top"><span class="feature-icon">◎</span><span class="ff-badge success">Analyse automatique</span></div>
          <h3>Un dossier entier devient un espace de travail.</h3>
          <p>FileFlow groupe images, PDF, Office, médias et archives, conserve l’arborescence et peut appliquer une action différente à chaque famille.</p>
          <div class="mini-tree">
            <span>Images <b>125</b></span><span>PDF <b>18</b></span><span>Documents <b>42</b></span><span>Archives <b>7</b></span>
          </div>
        </article>
        <article class="feature-card ff-card">
          <span class="feature-icon">⌁</span><h3>Confidentialité intégrée</h3><p>Détection et retrait des métadonnées GPS/EXIF avant partage.</p>
        </article>
        <article class="feature-card ff-card">
          <span class="feature-icon">Aa</span><h3>OCR & documents</h3><p>Scans recherchables, texte extractible et préparation de PDF propres.</p>
        </article>
        <article class="feature-card ff-card">
          <span class="feature-icon">⚡</span><h3>Scheduler adaptatif</h3><p>CPU, RAM et I/O sont arbitrés pour garder l’application fluide pendant les tâches lourdes.</p>
        </article>
        <article class="feature-card ff-card">
          <span class="feature-icon">↯</span><h3>Recettes</h3><p>Enchaînez conversion, nettoyage, renommage et destination en une seule action.</p>
        </article>
      </section>

      @if (capabilities.health(); as health) {
        <footer class="runtime-strip">
          <div><span class="status-pulse"></span><strong>FileFlow {{ health.version }}</strong></div>
          <span>{{ health.cpuThreads }} threads CPU</span>
          <span>{{ health.scheduler.budget.memoryMb }} Mo de budget RAM</span>
          <span>{{ capabilities.engineReadyCount() }}/{{ capabilities.engines().length }} moteurs disponibles</span>
          <span>{{ health.os }} · {{ health.architecture }}</span>
        </footer>
      }
    </div>
  `,
  styles: [`
    :host { display:block; }.home-shell { max-width: 1240px; margin: 0 auto; }.hero { display:grid; grid-template-columns:minmax(0,1fr) 310px; gap:42px; align-items:end; }.hero-copy { max-width:820px; }.hero h1 { margin:0; color:var(--text-strong); font-size:clamp(42px,6vw,72px); line-height:.98; letter-spacing:-.058em; }.hero h1 span { color:var(--accent); }.hero-lead { max-width:720px; margin:20px 0 0; color:var(--text-muted); font-size:16px; line-height:1.72; }.hero-trust { display:flex; gap:12px; align-items:flex-start; padding:16px; }.trust-orb { width:34px; height:34px; flex:none; display:grid; place-items:center; border-radius:11px; background:var(--success-soft); color:var(--success); font-size:10px; }.hero-trust strong,.hero-trust span { display:block; }.hero-trust strong { font-size:12px; }.hero-trust div span { margin-top:4px; color:var(--text-muted); font-size:10px; line-height:1.45; }
    .drop-zone { position:relative; overflow:hidden; min-height:272px; margin-top:38px; display:grid; place-items:center; align-content:center; gap:10px; padding:30px; border:1.5px dashed var(--border-strong); border-radius:24px; background:linear-gradient(180deg,var(--surface-1),color-mix(in srgb,var(--surface-1) 76%,var(--accent-soft))); text-align:center; box-shadow:var(--shadow-sm); }.drop-zone:hover { border-color:color-mix(in srgb,var(--accent) 42%,var(--border)); }.drop-zone.busy { border-color:var(--accent); }.drop-glow { position:absolute; width:420px; height:170px; top:-120px; border-radius:50%; background:color-mix(in srgb,var(--accent) 13%,transparent); filter:blur(30px); pointer-events:none; }.drop-symbol { width:58px; height:58px; display:grid; place-items:center; z-index:1; border-radius:18px; background:var(--accent-soft); color:var(--accent); font-size:31px; box-shadow:inset 0 0 0 1px color-mix(in srgb,var(--accent) 12%,transparent); }.drop-copy { z-index:1; }.drop-copy strong,.drop-copy span { display:block; }.drop-copy strong { color:var(--text-strong); font-size:16px; letter-spacing:-.02em; }.drop-copy span { margin-top:5px; color:var(--text-muted); font-size:12px; }.drop-actions { z-index:1; display:flex; gap:9px; margin-top:8px; }.format-hint { z-index:1; display:flex; flex-wrap:wrap; justify-content:center; gap:5px; margin-top:10px; }.format-hint span { padding:4px 7px; border-radius:6px; background:var(--surface-2); color:var(--text-faint); font-size:9px; font-weight:750; }.scan-line { width:min(420px,70%); height:5px; margin-top:10px; overflow:hidden; border-radius:99px; background:var(--surface-3); }.scan-line span { display:block; width:36%; height:100%; border-radius:inherit; background:var(--accent); animation:scan 1.2s ease-in-out infinite alternate; }@keyframes scan { to { transform:translateX(175%); } }
    .section-head { margin:42px 0 15px; display:flex; justify-content:space-between; gap:20px; align-items:end; }.section-head h2 { margin:0; font-size:24px; letter-spacing:-.035em; }.section-head > span { color:var(--text-faint); font-size:10px; }.action-grid { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:10px; }.action-card { min-width:0; min-height:104px; display:grid; grid-template-columns:42px minmax(0,1fr); grid-template-rows:1fr auto; gap:8px 11px; padding:14px; color:var(--text); text-align:left; transition:transform var(--transition),border-color var(--transition),background var(--transition); }.action-card:hover { transform:translateY(-2px); border-color:color-mix(in srgb,var(--accent) 26%,var(--border)); }.action-mark { grid-row:1/3; width:40px; height:40px; display:grid; place-items:center; border-radius:12px; background:var(--accent-soft); color:var(--accent); font-size:11px; font-weight:900; }.action-mark[data-category='pdf'] { background:#fff0ef; color:#c14f46; }.action-mark[data-category='image'] { background:#eaf6ff; color:#2875bb; }.action-mark[data-category='archive'] { background:var(--warning-soft); color:var(--warning); }.action-mark[data-category='privacy'] { background:var(--success-soft); color:var(--success); }.action-content { min-width:0; }.action-content strong,.action-content small { display:block; }.action-content strong { font-size:12px; letter-spacing:-.015em; }.action-content small { margin-top:4px; display:-webkit-box; overflow:hidden; color:var(--text-muted); font-size:10px; line-height:1.4; -webkit-box-orient:vertical; -webkit-line-clamp:2; }.action-state { color:var(--success); font-size:9px; font-weight:850; text-transform:uppercase; letter-spacing:.06em; }.action-state.missing { color:var(--warning); }
    .feature-grid { margin-top:34px; display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); gap:10px; }.feature-card { min-height:188px; padding:19px; }.feature-card.feature-large { grid-column:span 2; grid-row:span 2; min-height:386px; background:linear-gradient(145deg,var(--surface-1),color-mix(in srgb,var(--accent-soft) 42%,var(--surface-1))); }.feature-top { display:flex; justify-content:space-between; align-items:center; }.feature-icon { width:38px; height:38px; display:grid; place-items:center; border-radius:11px; background:var(--surface-2); color:var(--accent); font-weight:850; }.feature-card h3 { margin:20px 0 8px; font-size:16px; letter-spacing:-.025em; }.feature-large h3 { max-width:440px; margin-top:54px; font-size:30px; line-height:1.05; letter-spacing:-.045em; }.feature-card p { margin:0; color:var(--text-muted); font-size:11px; line-height:1.6; }.feature-large p { max-width:500px; font-size:13px; }.mini-tree { margin-top:28px; display:grid; grid-template-columns:repeat(2,1fr); gap:7px; }.mini-tree span { display:flex; justify-content:space-between; padding:10px 11px; border:1px solid var(--border); border-radius:9px; background:color-mix(in srgb,var(--surface-1) 70%,transparent); color:var(--text-muted); font-size:10px; }.mini-tree b { color:var(--text); }
    .runtime-strip { margin-top:28px; min-height:42px; display:flex; flex-wrap:wrap; align-items:center; gap:17px; padding:0 5px; color:var(--text-faint); font-size:9px; }.runtime-strip div { display:flex; align-items:center; gap:7px; color:var(--text-muted); }.status-pulse { width:7px; height:7px; border-radius:50%; background:var(--success); box-shadow:0 0 0 4px color-mix(in srgb,var(--success) 12%,transparent); }
    @media(max-width:1100px){.action-grid{grid-template-columns:repeat(2,1fr)}.feature-grid{grid-template-columns:repeat(2,1fr)}.hero{grid-template-columns:1fr}.hero-trust{display:none}}@media(max-width:680px){.hero h1{font-size:42px}.drop-zone{min-height:300px}.drop-actions{flex-direction:column;width:min(290px,100%)}.section-head>span{display:none}.action-grid,.feature-grid{grid-template-columns:1fr}.feature-card.feature-large{grid-column:auto;grid-row:auto;min-height:300px}.feature-large h3{margin-top:30px;font-size:26px}.runtime-strip{display:none}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  protected readonly store = inject(WorkspaceStore);
  protected readonly capabilities = inject(CapabilityStore);
  private readonly router = inject(Router);

  protected featuredActions(): ActionDescriptor[] {
    return this.capabilities.featuredActions().slice(0, 8);
  }

  protected async chooseFiles(): Promise<void> {
    const paths = await this.store.pickFiles();
    await this.begin(paths);
  }

  protected async chooseDirectories(): Promise<void> {
    const paths = await this.store.pickDirectories();
    await this.begin(paths);
  }

  protected async startAction(action: ActionDescriptor): Promise<void> {
    this.store.setPendingAction(action.id);
    if (this.store.hasWorkspace()) {
      this.store.openAction(action.id);
      await this.router.navigate(['/workspace']);
      return;
    }
    const paths = await this.store.pickFiles();
    await this.begin(paths);
  }

  protected actionMark(category: OperationCategory): string {
    const marks: Record<OperationCategory, string> = {
      convert: '↔', pdf: 'PDF', image: 'IMG', document: 'DOC', media: '▶', archive: 'ZIP',
      extract: 'Aa', organize: '▦', privacy: '◌', optimize: '↓',
    };
    return marks[category];
  }

  protected formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} o`;
    const units = ['Ko', 'Mo', 'Go', 'To'];
    let value = bytes / 1024;
    let index = 0;
    while (index < units.length - 1 && value >= 1024) { value /= 1024; index += 1; }
    return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
  }

  private async begin(paths: string[]): Promise<void> {
    if (!paths.length) return;
    await this.router.navigate(['/workspace']);
    await this.store.start(paths);
  }
}
