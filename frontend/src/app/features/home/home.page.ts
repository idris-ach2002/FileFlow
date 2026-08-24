import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

@Component({
  selector: 'ff-home-page',
  template: `
    <div class="ff-page simple-home">
      <header class="home-heading">
        <p class="eyebrow">BONJOUR {{ auth.profile()?.firstName || auth.profile()?.displayName || '' }}</p>
        <h1>Qu’est-ce que vous<br />voulez faire&nbsp;?</h1>
        <p class="lead">Commencez simplement par votre fichier. FileFlow le reconnaît et vous guide sans vous montrer la technique.</p>
      </header>

      <section class="home-card" aria-labelledby="home-drop-title">
        <div class="step-badge"><span>1</span><strong>Accueil</strong></div>

        <div class="drop-zone" [class.busy]="store.busy()">
          @if (store.busy()) {
            <div class="upload-icon pulse">···</div>
            <h2 id="home-drop-title">FileFlow regarde vos fichiers…</h2>
            <p>{{ store.stats().discovered }} élément(s) détecté(s)</p>
          } @else {
            <div class="upload-icon" aria-hidden="true">
              <span class="cloud">☁</span><span class="arrow">↑</span>
            </div>
            <h2 id="home-drop-title">Glissez-déposez votre fichier ici</h2>
            <span class="or">ou</span>
            <button class="primary-choice" type="button" (click)="chooseFiles()">
              <span class="folder-icon">▱</span> Choisir un fichier
            </button>
            <button class="folder-choice" type="button" (click)="chooseDirectories()">Choisir un dossier</button>
          }
        </div>

        <div class="quick-label">Catégories rapides</div>
        <div class="quick-categories" aria-label="Catégories de fichiers prises en charge">
          <button type="button" (click)="chooseFiles()"><span class="cat-icon pdf">▤</span><strong>PDF</strong></button>
          <button type="button" (click)="chooseFiles()"><span class="cat-icon image">▧</span><strong>Images</strong></button>
          <button type="button" (click)="chooseFiles()"><span class="cat-icon video">▶</span><strong>Vidéo</strong></button>
          <button type="button" (click)="chooseFiles()"><span class="cat-icon archive">▰</span><strong>Archives</strong></button>
        </div>

      </section>

      <aside class="home-trust">
        <button type="button" (click)="router.navigate(['/advanced'])"><span>✦</span><strong>Vous connaissez déjà les formats&nbsp;?</strong><small>Ouvrir l’espace expert</small><b>→</b></button>
      </aside>
    </div>
  `,
  styles: [`
    :host{display:block}.simple-home{max-width:1120px;margin:0 auto;padding-top:22px}.home-heading{max-width:820px}.eyebrow{margin:0 0 16px;color:var(--text);font-size:13px;font-weight:820;letter-spacing:.04em}.home-heading h1{margin:0;font-size:clamp(46px,6.2vw,78px);line-height:.98;letter-spacing:-.065em;font-weight:850}.lead{max-width:720px;margin:20px 0 0;color:var(--text-muted);font-size:17px;line-height:1.6}.home-card{margin-top:38px;padding:22px;border:1px solid var(--border);border-radius:28px;background:var(--surface-1);box-shadow:var(--shadow-sm)}.step-badge{display:flex;align-items:center;gap:10px;margin-bottom:18px}.step-badge span{width:30px;height:30px;display:grid;place-items:center;border-radius:50%;background:var(--accent-soft);color:var(--accent);font-weight:900}.step-badge strong{font-size:17px}.drop-zone{min-height:310px;display:flex;flex-direction:column;align-items:center;justify-content:center;padding:34px;border:1.5px dashed color-mix(in srgb,var(--accent) 32%,var(--border-strong));border-radius:22px;background:linear-gradient(145deg,var(--surface-1),color-mix(in srgb,var(--accent-soft) 32%,var(--surface-1)));text-align:center;transition:var(--transition);animation:dropGlow 5s ease-in-out infinite}.drop-zone:hover{border-color:color-mix(in srgb,var(--accent) 68%,var(--border));box-shadow:inset 0 0 0 1px color-mix(in srgb,var(--accent) 8%,transparent)}.upload-icon{position:relative;width:78px;height:62px;margin-bottom:14px;color:var(--accent)}.cloud{font-size:58px;line-height:1}.arrow{position:absolute;left:50%;bottom:1px;transform:translateX(-50%);width:28px;height:28px;display:grid;place-items:center;border-radius:50%;background:var(--surface-1);font-size:24px;font-weight:900}.drop-zone h2{margin:0;font-size:20px;letter-spacing:-.03em}.drop-zone>p,.or{margin-top:8px;color:var(--text-muted);font-size:13px}.primary-choice{position:relative;min-width:230px;min-height:52px;display:flex;align-items:center;justify-content:center;gap:10px;margin-top:12px;overflow:hidden;border:0;border-radius:12px;background:linear-gradient(135deg,var(--accent),var(--violet));color:white;font-size:16px;font-weight:800;box-shadow:0 13px 28px color-mix(in srgb,var(--accent) 22%,transparent);transition:var(--transition);animation:buttonBreathe 3.2s ease-in-out infinite}.primary-choice:hover{transform:translateY(-1px);box-shadow:0 16px 34px color-mix(in srgb,var(--accent) 27%,transparent)}.folder-icon{font-size:20px}.folder-choice{margin-top:10px;border:0;background:transparent;color:var(--text-muted);font-size:14px;font-weight:700}.folder-choice:hover{color:var(--accent)}.quick-label{margin:20px 2px 10px;color:var(--text-muted);font-size:13px;font-weight:760}.quick-categories{display:grid;grid-template-columns:repeat(4,1fr);gap:10px}.quick-categories button{min-height:92px;display:grid;place-items:center;align-content:center;gap:8px;border:1px solid var(--border);border-radius:16px;background:var(--surface-2);color:var(--text);transition:var(--transition)}.quick-categories button:hover{transform:translateY(-2px);border-color:var(--border-strong);background:var(--surface-1);box-shadow:var(--shadow-sm)}.quick-categories strong{font-size:13px}.cat-icon{width:38px;height:38px;display:grid;place-items:center;border-radius:11px;font-size:16px;font-weight:900}.cat-icon.pdf{background:var(--danger-soft);color:var(--danger)}.cat-icon.image{background:var(--success-soft);color:var(--success)}.cat-icon.video{background:var(--accent-soft);color:var(--accent)}.cat-icon.archive{background:var(--warning-soft);color:var(--warning)}.home-trust{display:flex;justify-content:flex-end;margin-top:14px}.home-trust>button{width:min(100%,470px);min-height:70px;display:grid;grid-template-columns:34px 1fr auto;align-items:center;gap:9px;padding:12px 15px;border:1px solid var(--border);border-radius:17px;background:var(--surface-1);color:var(--text);text-align:left}.home-trust strong,.home-trust small{display:block}.home-trust strong{font-size:14px}.home-trust small{margin-top:2px;color:var(--text-muted);font-size:12px}.home-trust button>span,.home-trust button>b{color:var(--accent)}.home-trust button>b{font-size:20px}.pulse{animation:pulse 1s infinite alternate}@keyframes pulse{to{opacity:.45;transform:scale(.97)}}@keyframes buttonBreathe{50%{box-shadow:0 16px 40px color-mix(in srgb,var(--accent) 34%,transparent)}}@keyframes dropGlow{50%{border-color:color-mix(in srgb,var(--accent) 52%,var(--border));background:linear-gradient(145deg,var(--surface-1),color-mix(in srgb,var(--accent-soft) 54%,var(--surface-1)))}}@media(max-width:760px){.simple-home{padding-top:8px}.home-heading h1{font-size:44px}.lead{font-size:15px}.home-card{margin-top:26px;padding:15px}.drop-zone{min-height:270px}.quick-categories{grid-template-columns:repeat(2,1fr)}.home-trust{display:block}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  protected readonly auth = inject(AuthStore);
  protected readonly store = inject(WorkspaceStore);
  protected readonly router = inject(Router);

  protected async chooseFiles(): Promise<void> {
    const paths = await this.store.pickFiles();
    if (paths.length && await this.store.start(paths)) await this.router.navigate(['/conversion']);
  }

  protected async chooseDirectories(): Promise<void> {
    const paths = await this.store.pickDirectories();
    if (paths.length && await this.store.start(paths)) await this.router.navigate(['/conversion']);
  }
}
