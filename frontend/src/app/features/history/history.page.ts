import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { HistoryEntry } from '../../core/ipc/tauri.models';
import { HistoryStore } from './history.store';

@Component({
  selector: 'ff-history-page',
  template: `
    <div class="history-shell">
      <header class="history-head">
        <div><p class="ff-kicker">HISTORIQUE LOCAL</p><h1>Vos opérations, sans vos fichiers.</h1><p>FileFlow conserve seulement les métadonnées utiles dans SQLite. Aucun document, image ou contenu OCR n’est copié dans l’historique.</p></div>
        <button class="ff-button secondary" type="button" [disabled]="store.loading()" (click)="store.load(true)">↻ Actualiser</button>
      </header>

      <section class="history-stats">
        <article class="ff-card"><span>Opérations</span><strong>{{ store.entries().length }}</strong><small>{{ store.completed() }} terminée(s)</small></article>
        <article class="ff-card"><span>Données traitées</span><strong>{{ formatBytes(store.totalInputBytes()) }}</strong><small>entrées cumulées</small></article>
        <article class="ff-card accent"><span>Réduction observée</span><strong>{{ formatBytes(store.savedBytes()) }}</strong><small>sur les sorties mesurables</small></article>
      </section>

      @if (store.error()) { <div class="history-error ff-card">{{ store.error() }}</div> }
      @if (store.loading() && !store.entries().length) {
        <section class="history-loading ff-card"><span></span><strong>Lecture de l’historique…</strong></section>
      } @else if (store.entries().length) {
        <section class="history-list ff-card">
          <div class="list-head"><strong>Activité récente</strong><span>{{ store.entries().length }} entrée(s)</span></div>
          @for (entry of store.entries(); track entry.id) {
            <article class="history-row">
              <div class="history-mark" [class.failed]="entry.status === 'failed'" [class.cancelled]="entry.status === 'cancelled'">{{ mark(entry) }}</div>
              <div class="history-main"><strong>{{ actionTitle(entry.actionId) }}</strong><span>{{ entry.inputCount }} entrée(s) → {{ entry.outputCount }} sortie(s)</span><small>{{ entry.destination || 'Aucune sortie finale' }}</small></div>
              <div class="history-size"><strong>{{ formatBytes(entry.inputBytes) }}</strong><span>→ {{ formatBytes(entry.outputBytes) }}</span>@if (saved(entry) > 0) { <small>−{{ formatBytes(saved(entry)) }}</small> }</div>
              <div class="history-meta"><strong>{{ duration(entry.durationMs) }}</strong><span>{{ date(entry.createdAt) }}</span><small [class.bad]="entry.status === 'failed'">{{ statusLabel(entry.status) }}</small></div>
            </article>
          }
        </section>
      } @else {
        <section class="history-empty ff-card"><div class="history-art"><span>↺</span><i></i><i></i><i></i></div><h2>Aucune opération enregistrée</h2><p>Lancez une conversion dans l’espace Fichiers. Le résultat apparaîtra ici automatiquement.</p></section>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.history-shell{max-width:1120px;margin:0 auto}.history-head{display:flex;justify-content:space-between;align-items:flex-end;gap:28px}.history-head>div{max-width:760px}.history-head h1{margin:0;font-size:48px;letter-spacing:-.05em}.history-head p:last-child{margin:10px 0 0;color:var(--text-muted);font-size:12px;line-height:1.6}.history-stats{display:grid;grid-template-columns:repeat(3,1fr);gap:9px;margin-top:28px}.history-stats article{padding:16px}.history-stats span,.history-stats strong,.history-stats small{display:block}.history-stats span{color:var(--text-faint);font-size:10px;font-weight:850;text-transform:uppercase}.history-stats strong{margin-top:7px;font-size:23px;letter-spacing:-.04em}.history-stats small{margin-top:3px;color:var(--text-muted);font-size:10px}.history-stats .accent{background:linear-gradient(145deg,var(--accent-soft),var(--bg-elevated))}.history-list{margin-top:12px;overflow:hidden}.list-head{display:flex;justify-content:space-between;padding:14px 16px;border-bottom:1px solid var(--border)}.list-head strong{font-size:11px}.list-head span{color:var(--text-faint);font-size:11px}.history-row{display:grid;grid-template-columns:40px minmax(0,1fr) 120px 128px;align-items:center;gap:12px;padding:11px 16px;border-bottom:1px solid var(--border)}.history-row:last-child{border-bottom:0}.history-mark{width:36px;height:36px;display:grid;place-items:center;border-radius:10px;background:var(--success-soft);color:var(--success);font-size:11px;font-weight:900}.history-mark.failed{background:var(--danger-soft);color:var(--danger)}.history-mark.cancelled{background:var(--surface-2);color:var(--text-muted)}.history-main,.history-size,.history-meta{min-width:0}.history-main strong,.history-main span,.history-main small,.history-size strong,.history-size span,.history-size small,.history-meta strong,.history-meta span,.history-meta small{display:block}.history-main strong{font-size:10px}.history-main span,.history-main small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.history-main span{margin-top:3px;color:var(--text-muted);font-size:10px}.history-main small{margin-top:3px;color:var(--text-faint);font-size:10px}.history-size{text-align:right}.history-size strong{font-size:11px}.history-size span{color:var(--text-muted);font-size:10px}.history-size small{margin-top:3px;color:var(--success);font-size:10px;font-weight:800}.history-meta{text-align:right}.history-meta strong{font-size:11px}.history-meta span{margin-top:2px;color:var(--text-muted);font-size:10px}.history-meta small{margin-top:4px;color:var(--success);font-size:10px;text-transform:capitalize}.history-meta small.bad{color:var(--danger)}.history-empty{margin-top:24px;min-height:360px;display:grid;justify-items:center;align-content:center;padding:38px;text-align:center}.history-art{position:relative;width:170px;height:88px;margin-bottom:16px}.history-art span{position:absolute;left:0;top:10px;width:58px;height:58px;display:grid;place-items:center;border-radius:18px;background:var(--accent-soft);color:var(--accent);font-size:25px}.history-art i{position:absolute;right:0;width:92px;height:15px;border-radius:6px;background:var(--surface-2)}.history-art i:nth-child(2){top:8px}.history-art i:nth-child(3){top:34px;width:74px}.history-art i:nth-child(4){top:60px;width:84px}.history-empty h2{margin:0;font-size:20px}.history-empty p{max-width:520px;color:var(--text-muted);font-size:10px}.history-loading{margin-top:24px;min-height:180px;display:grid;place-items:center;align-content:center;gap:10px}.history-loading span{width:24px;height:24px;border:2px solid var(--border);border-top-color:var(--accent);border-radius:50%;animation:spin .8s linear infinite}.history-error{margin-top:15px;padding:12px;color:var(--danger)}@keyframes spin{to{transform:rotate(360deg)}}@media(max-width:800px){.history-head{align-items:flex-start}.history-head h1{font-size:38px}.history-stats{grid-template-columns:1fr 1fr}.history-stats article:last-child{grid-column:1/-1}.history-row{grid-template-columns:36px minmax(0,1fr) 90px}.history-meta{display:none}}@media(max-width:540px){.history-head{display:block}.history-head .ff-button{margin-top:14px}.history-stats{grid-template-columns:1fr}.history-stats article:last-child{grid-column:auto}.history-row{grid-template-columns:34px minmax(0,1fr)}.history-size{display:none}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HistoryPage {
  protected readonly store = inject(HistoryStore);
  private readonly capabilities = inject(CapabilityStore);

  constructor() { this.store.load(); }

  protected actionTitle(actionId: string): string { return this.capabilities.action(actionId)?.title ?? actionId; }
  protected mark(entry: HistoryEntry): string { return entry.status === 'completed' ? 'OK' : entry.status === 'cancelled' ? '—' : '!'; }
  protected saved(entry: HistoryEntry): number {
    if (!['pdf-compress', 'media-compress', 'image-optimize'].includes(entry.actionId) || entry.status !== 'completed') return 0;
    return Math.max(0, entry.inputBytes - entry.outputBytes);
  }
  protected statusLabel(status: string): string { return status === 'completed' ? 'Terminé' : status === 'cancelled' ? 'Annulé' : status === 'failed' ? 'Échec' : status; }
  protected duration(ms: number): string { return ms < 1000 ? `${ms} ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`; }
  protected date(value: string): string { return new Intl.DateTimeFormat('fr-FR',{day:'2-digit',month:'short',hour:'2-digit',minute:'2-digit'}).format(new Date(value)); }
  protected formatBytes(bytes: number): string { if(bytes<1024)return`${bytes} o`;const units=['Ko','Mo','Go','To'];let value=bytes/1024,index=0;while(index<units.length-1&&value>=1024){value/=1024;index+=1;}return`${value>=10?value.toFixed(0):value.toFixed(1)} ${units[index]}`; }
}
