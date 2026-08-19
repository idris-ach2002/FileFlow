import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { PreferencesService } from '../../core/preferences/preferences.service';
import { EngineProbe } from '../../core/ipc/tauri.models';

@Component({
  selector: 'ff-settings-page',
  template: `
    <div class="settings-shell">
      <header><p class="ff-kicker">PARAMÈTRES</p><h1>FileFlow, à votre façon.</h1><p>Les réglages simples restent devant. Les détails techniques restent accessibles sans polluer l’usage quotidien.</p></header>

      <div class="settings-grid">
        <section class="settings-main">
          <article class="setting-card ff-card">
            <div class="setting-head"><div><strong>Apparence</strong><span>Thème et densité de l’interface.</span></div></div>
            <div class="setting-row"><label>Thème</label><div class="segmented"><button [class.active]="prefs.theme() === 'system'" (click)="prefs.theme.set('system')">Système</button><button [class.active]="prefs.theme() === 'light'" (click)="prefs.theme.set('light')">Clair</button><button [class.active]="prefs.theme() === 'dark'" (click)="prefs.theme.set('dark')">Sombre</button></div></div>
            <div class="setting-row"><label>Densité</label><div class="segmented"><button [class.active]="prefs.density() === 'comfortable'" (click)="prefs.density.set('comfortable')">Confort</button><button [class.active]="prefs.density() === 'compact'" (click)="prefs.density.set('compact')">Compact</button></div></div>
          </article>

          <article class="setting-card ff-card">
            <div class="setting-head"><div><strong>Résultats</strong><span>Comportement par défaut pour les nouveaux fichiers.</span></div></div>
            <div class="setting-row"><label>Destination</label><div class="segmented"><button [class.active]="prefs.destination() === 'subfolder'" (click)="prefs.destination.set('subfolder')">Sous-dossier</button><button [class.active]="prefs.destination() === 'sameFolder'" (click)="prefs.destination.set('sameFolder')">Même dossier</button><button [class.active]="prefs.destination() === 'ask'" (click)="prefs.destination.set('ask')">Demander</button></div></div>
            <label class="switch-row"><div><strong>Conserver l’arborescence</strong><span>Reproduit les sous-dossiers lors d’un traitement de dossier complet.</span></div><input type="checkbox" [checked]="prefs.preserveTree()" (change)="prefs.preserveTree.set(!prefs.preserveTree())" /></label>
            <label class="switch-row"><div><strong>Afficher les fichiers cachés</strong><span>Ils peuvent toujours être filtrés dans le Workspace.</span></div><input type="checkbox" [checked]="prefs.showHidden()" (change)="prefs.showHidden.set(!prefs.showHidden())" /></label>
          </article>

          <article class="setting-card ff-card">
            <div class="setting-head"><div><strong>Performances</strong><span>Budget détecté par le scheduler Rust.</span></div><span class="ff-badge success">Adaptatif</span></div>
            @if (capabilities.health(); as health) {
              <div class="performance-grid"><div><span>CPU</span><strong>{{ health.scheduler.budget.cpuTokens }}</strong><small>tokens / {{ health.cpuThreads }} threads</small></div><div><span>RAM</span><strong>{{ formatMemory(health.scheduler.budget.memoryMb) }}</strong><small>budget FileFlow</small></div><div><span>I/O</span><strong>{{ health.scheduler.budget.ioTokens }}</strong><small>opérations parallèles</small></div></div>
              <p class="performance-note">Les moteurs déjà multithreadés, comme FFmpeg ou libvips, reçoivent un quota dédié afin d’éviter la sur-saturation du processeur.</p>
            } @else { <div class="skeleton-line"></div> }
          </article>
        </section>

        <aside class="engine-card ff-card">
          <div class="engine-head"><div><p class="ff-kicker">MOTEURS</p><h2>Diagnostic local</h2></div><button class="refresh" type="button" (click)="refresh()">↻</button></div>
          <div class="engine-summary"><strong>{{ capabilities.engineReadyCount() }}/{{ capabilities.engines().length }}</strong><span>moteurs disponibles</span></div>
          <div class="engine-list">
            @for (engine of capabilities.engines(); track engine.id) {
              <div class="engine-row"><span class="engine-dot" [class.missing]="!engine.available"></span><div><strong>{{ engine.displayName }}</strong><small>{{ engine.available ? executableLabel(engine) : 'Non détecté' }}</small></div><span class="engine-profile">{{ profileLabel(engine) }}</span></div>
            } @empty { <div class="engine-empty">Initialisation du diagnostic…</div> }
          </div>
          <div class="engine-foot"><span>Les fonctions dont le moteur manque restent visibles mais sont désactivées.</span></div>
        </aside>
      </div>
    </div>
  `,
  styles: [`
    :host{display:block}.settings-shell{max-width:1180px;margin:0 auto}header{max-width:760px}header h1{margin:0;font-size:48px;letter-spacing:-.05em}header>p:last-child{margin:10px 0 0;color:var(--text-muted);font-size:13px;line-height:1.6}.settings-grid{margin-top:30px;display:grid;grid-template-columns:minmax(0,1fr) 360px;gap:14px;align-items:start}.settings-main{display:grid;gap:12px}.setting-card{padding:18px}.setting-head{display:flex;justify-content:space-between;gap:12px;align-items:flex-start;padding-bottom:14px;border-bottom:1px solid var(--border)}.setting-head strong,.setting-head span{display:block}.setting-head strong{font-size:13px}.setting-head div>span{margin-top:4px;color:var(--text-muted);font-size:10px}.setting-row,.switch-row{min-height:62px;display:grid;grid-template-columns:190px minmax(0,1fr);align-items:center;gap:12px;border-bottom:1px solid var(--border)}.setting-row:last-child,.switch-row:last-child{border-bottom:0}.setting-row>label{color:var(--text-muted);font-size:10px;font-weight:750}.segmented{display:flex;justify-self:end;padding:3px;border-radius:9px;background:var(--surface-2)}.segmented button{min-height:31px;padding:0 12px;border:0;border-radius:7px;background:transparent;color:var(--text-muted);font-size:11px;font-weight:750}.segmented button.active{background:var(--surface-1);color:var(--text);box-shadow:var(--shadow-sm)}.switch-row{grid-template-columns:minmax(0,1fr) auto;cursor:pointer}.switch-row strong,.switch-row span{display:block}.switch-row strong{font-size:10px}.switch-row span{margin-top:3px;color:var(--text-muted);font-size:11px}.switch-row input{width:36px;height:20px;accent-color:var(--accent)}.performance-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-top:14px}.performance-grid>div{padding:12px;border-radius:10px;background:var(--bg-elevated)}.performance-grid span,.performance-grid strong,.performance-grid small{display:block}.performance-grid span{color:var(--text-faint);font-size:10px;font-weight:850;text-transform:uppercase}.performance-grid strong{margin-top:5px;font-size:20px}.performance-grid small{margin-top:3px;color:var(--text-muted);font-size:10px}.performance-note{margin:12px 0 0;color:var(--text-muted);font-size:11px;line-height:1.55}.engine-card{position:sticky;top:24px;overflow:hidden}.engine-head{display:flex;justify-content:space-between;align-items:flex-start;padding:17px 17px 8px}.engine-head h2{margin:0;font-size:18px;letter-spacing:-.03em}.refresh{width:32px;height:32px;border:1px solid var(--border);border-radius:9px;background:var(--surface-2);color:var(--text-muted)}.engine-summary{display:flex;align-items:baseline;gap:7px;padding:4px 17px 14px}.engine-summary strong{font-size:28px;letter-spacing:-.04em}.engine-summary span{color:var(--text-muted);font-size:11px}.engine-list{border-top:1px solid var(--border)}.engine-row{min-height:55px;display:grid;grid-template-columns:10px minmax(0,1fr) auto;align-items:center;gap:9px;padding:8px 14px;border-bottom:1px solid var(--border)}.engine-dot{width:7px;height:7px;border-radius:50%;background:var(--success);box-shadow:0 0 0 3px color-mix(in srgb,var(--success) 10%,transparent)}.engine-dot.missing{background:var(--text-faint);box-shadow:none}.engine-row strong,.engine-row small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.engine-row strong{font-size:10px}.engine-row small{max-width:190px;margin-top:3px;color:var(--text-faint);font-size:10px}.engine-profile{color:var(--text-faint);font-size:10px;font-weight:750}.engine-foot{padding:11px 14px;color:var(--text-muted);font-size:10px;line-height:1.45;background:var(--bg-elevated)}.engine-empty{padding:30px;color:var(--text-muted);font-size:10px;text-align:center}@media(max-width:980px){.settings-grid{grid-template-columns:1fr}.engine-card{position:static}.engine-list{display:grid;grid-template-columns:repeat(2,1fr)}.engine-row:nth-child(odd){border-right:1px solid var(--border)}}@media(max-width:620px){header h1{font-size:38px}.setting-row{grid-template-columns:1fr;gap:6px;padding:10px 0}.segmented{justify-self:start;width:100%}.segmented button{flex:1}.performance-grid{grid-template-columns:1fr}.engine-list{grid-template-columns:1fr}.engine-row:nth-child(odd){border-right:0}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsPage {
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly prefs = inject(PreferencesService);
  protected refresh(): void { void this.capabilities.refreshEngines(); }
  protected executableLabel(engine: EngineProbe): string { return engine.executable?.split('/').pop() ?? 'Disponible'; }
  protected profileLabel(engine: EngineProbe): string { const cpu=engine.resourceProfile.cpuWeight; return cpu>=5?'intensif':cpu>=3?'moyen':'léger'; }
  protected formatMemory(megabytes: number): string { return megabytes >= 1024 ? `${(megabytes/1024).toFixed(megabytes%1024===0?0:1)} Go` : `${megabytes} Mo`; }
}
