import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { TauriBridgeService } from '../../core/ipc/tauri-bridge.service';
import {
  DuplicateCleanupPlan,
  OrganizationPreview,
  RenamePreview,
  RenameRule,
} from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

type OrganizeMode = 'rename' | 'classify' | 'duplicates';
type OrganizationMode = 'type' | 'date' | 'typeDate';
type DuplicateStrategy = 'newest' | 'oldest' | 'shortestPath';

@Component({
  selector: 'ff-organize-page',
  template: `
    <div class="organize-shell">
      <header class="organize-head">
        <div>
          <p class="ff-kicker">ORGANISER</p>
          <h1>Rangez beaucoup de fichiers sans prendre de risque.</h1>
          <p>FileFlow prépare toujours un aperçu. Le vrai plan est recalculé côté Rust juste avant l’application pour détecter les collisions de dernière seconde.</p>
        </div>
        @if (workspace.hasWorkspace()) {
          <span class="workspace-pill">{{ targetCountLabel() }}</span>
        }
      </header>

      @if (!workspace.hasWorkspace()) {
        <section class="empty ff-card">
          <div class="empty-mark">▦</div>
          <h2>Ajoutez d’abord vos fichiers</h2>
          <p>Le renommage, le classement et la recherche de doublons travaillent sur l’espace Fichiers courant.</p>
          <button class="ff-button" type="button" (click)="goWorkspace()">Ouvrir Fichiers</button>
        </section>
      } @else {
        <nav class="mode-tabs" aria-label="Outils d’organisation">
          <button type="button" [class.active]="mode()==='rename'" (click)="selectMode('rename')"><span>01</span><strong>Renommer</strong><small>Aperçu Avant → Après</small></button>
          <button type="button" [class.active]="mode()==='classify'" (click)="selectMode('classify')"><span>02</span><strong>Classer</strong><small>Type et/ou date</small></button>
          <button type="button" [class.active]="mode()==='duplicates'" (click)="selectMode('duplicates')"><span>03</span><strong>Doublons</strong><small>Comparer puis mettre à l’écart</small></button>
        </nav>

        @if (error()) { <div class="message error">{{ error() }}</div> }
        @if (notice()) { <div class="message success">{{ notice() }}</div> }

        @switch (mode()) {
          @case ('rename') {
            <section class="tool-grid">
              <aside class="controls ff-card">
                <div class="section-title"><span>RENOMMAGE MASSIF</span><strong>Construisez le nom voulu</strong></div>
                <label>Modèle de nom<input type="text" [value]="renameRule().template" (input)="updateRename('template',$any($event.target).value)" placeholder="{date}-{name}-{counter}" /></label>
                <div class="token-row">
                  @for (token of renameTokens; track token) { <button type="button" (click)="insertToken(token)">{{ token }}</button> }
                </div>
                <div class="two-col">
                  <label>Rechercher<input type="text" [value]="renameRule().search" (input)="updateRename('search',$any($event.target).value)" /></label>
                  <label>Remplacer par<input type="text" [value]="renameRule().replace" (input)="updateRename('replace',$any($event.target).value)" /></label>
                </div>
                <div class="two-col">
                  <label>Départ compteur<input type="number" min="0" max="999999" [value]="renameRule().counterStart" (input)="updateRenameNumber('counterStart',$any($event.target).value)" /></label>
                  <label>Nombre de chiffres<input type="number" min="1" max="12" [value]="renameRule().counterPadding" (input)="updateRenameNumber('counterPadding',$any($event.target).value)" /></label>
                </div>
                <label>Casse<select [value]="renameRule().caseMode" (change)="updateRename('caseMode',$any($event.target).value)"><option value="keep">Conserver</option><option value="lower">minuscules</option><option value="upper">MAJUSCULES</option><option value="title">Titre</option></select></label>
                <label class="check"><input type="checkbox" [checked]="renameRule().preserveExtension" (change)="updateRenameBool('preserveExtension',$any($event.target).checked)" /><span>Conserver l’extension d’origine</span></label>
                <button class="ff-button" type="button" [disabled]="busy()" (click)="previewRename()">{{ busy() ? 'Calcul…' : 'Prévisualiser' }}</button>
              </aside>

              <section class="preview ff-card">
                <div class="preview-head"><div><span>APERÇU TRANSACTIONNEL</span><strong>{{ renamePreview()?.changed ?? 0 }} changement(s)</strong></div>@if(renamePreview()?.conflicts){<b class="danger-pill">{{ renamePreview()?.conflicts }} collision(s)</b>}</div>
                @if (renamePreview(); as preview) {
                  <div class="summary-line"><span>{{ preview.total }} élément(s) analysé(s)</span>@if(preview.truncated){<small>Aperçu limité à {{ preview.items.length }} lignes pour préserver la fluidité.</small>}</div>
                  <div class="rename-list">
                    @for (item of preview.items; track item.assetId) {
                      <article [class.conflict]="item.conflict"><span class="before">{{ basename(item.source) }}</span><i>→</i><span class="after">{{ basename(item.target) }}</span>@if(item.warning){<small>{{ item.warning }}</small>}</article>
                    } @empty { <div class="preview-empty">Aucun changement avec cette règle.</div> }
                  </div>
                  <footer class="apply-bar"><span>Le plan complet sera recalculé avant l’écriture.</span><button class="ff-button" type="button" [disabled]="busy() || preview.conflicts>0 || preview.changed===0" (click)="applyRename()">Appliquer {{ preview.changed }} renommage(s)</button></footer>
                } @else { <div class="preview-placeholder"><span>A → B</span><strong>Aucun fichier n’est modifié avant votre validation.</strong><p>Choisissez un modèle puis cliquez sur Prévisualiser.</p></div> }
              </section>
            </section>
          }

          @case ('classify') {
            <section class="tool-grid">
              <aside class="controls ff-card">
                <div class="section-title"><span>CLASSEMENT INTELLIGENT</span><strong>Une arborescence simple et prévisible</strong></div>
                <label>Dossier de destination<div class="folder-field"><input readonly [value]="destination()" placeholder="Choisir un dossier…" /><button type="button" (click)="chooseDestination()">Choisir</button></div></label>
                <label>Organisation<select [value]="organizationMode()" (change)="organizationMode.set($any($event.target).value); organizationPreview.set(null)"><option value="type">Par type</option><option value="date">Par année / mois</option><option value="typeDate">Type → année → mois</option></select></label>
                <div class="explanation"><strong>Exemple</strong><code>{{ organizationExample() }}</code><span>La famille est déterminée par le contenu détecté, pas seulement par l’extension.</span></div>
                <button class="ff-button" type="button" [disabled]="busy() || !destination()" (click)="previewOrganization()">Prévisualiser le classement</button>
              </aside>

              <section class="preview ff-card">
                <div class="preview-head"><div><span>PLAN DE CLASSEMENT</span><strong>{{ organizationPreview()?.total ?? 0 }} fichier(s)</strong></div></div>
                @if (organizationPreview(); as preview) {
                  <div class="category-chips">@for(entry of categoryEntries(preview); track entry[0]){<span><b>{{ entry[1] }}</b>{{ entry[0] }}</span>}</div>
                  @if(preview.truncated){<div class="summary-line"><small>Aperçu limité à {{ preview.items.length }} lignes ; le plan complet reste côté Rust.</small></div>}
                  <div class="rename-list">
                    @for (item of preview.items; track item.assetId) {
                      <article><span class="before">{{ basename(item.source) }}</span><i>→</i><span class="after path">{{ shortTarget(item.target) }}</span>@if(item.conflictResolved){<small>Nom ajusté pour éviter une collision</small>}</article>
                    }
                  </div>
                  <footer class="apply-bar"><span>Les déplacements sont rollbackés si une étape échoue.</span><button class="ff-button" type="button" [disabled]="busy() || preview.total===0" (click)="applyOrganization()">Classer maintenant</button></footer>
                } @else { <div class="preview-placeholder"><span>⌁</span><strong>Choisissez où ranger les fichiers.</strong><p>FileFlow vous montre l’arborescence avant de déplacer quoi que ce soit.</p></div> }
              </section>
            </section>
          }

          @case ('duplicates') {
            <section class="tool-grid">
              <aside class="controls ff-card">
                <div class="section-title"><span>DOUBLONS CONFIRMÉS</span><strong>Hash complet avant décision</strong></div>
                <label>Fichier à conserver<select [value]="duplicateStrategy()" (change)="duplicateStrategy.set($any($event.target).value); duplicatePlan.set(null)"><option value="newest">Le plus récent</option><option value="oldest">Le plus ancien</option><option value="shortestPath">Le chemin le plus simple</option></select></label>
                <div class="safety-note"><b>Pas de suppression directe.</b><span>Les copies sont déplacées dans « Doublons à vérifier ». Vous pouvez les récupérer à tout moment.</span></div>
                <button class="ff-button" type="button" [disabled]="busy()" (click)="scanDuplicates()">{{ busy() ? 'Analyse complète…' : 'Analyser les doublons' }}</button>
              </aside>

              <section class="preview ff-card">
                <div class="preview-head"><div><span>COMPARAISON</span><strong>{{ duplicatePlan()?.groups?.length ?? 0 }} groupe(s)</strong></div>@if(duplicatePlan()){<b class="success-pill">{{ formatBytes(duplicatePlan()!.reclaimableBytes) }} récupérables</b>}</div>
                @if (duplicatePlan(); as plan) {
                  <div class="duplicate-list">
                    @for (group of plan.groups.slice(0, 80); track group.hash) {
                      <article><div class="keep"><span>À garder</span><strong>{{ basename(group.keepPath) }}</strong><small>{{ shortPath(group.keepPath) }}</small></div><div class="copies"><span>{{ group.quarantinePaths.length }} copie(s)</span>@for(path of group.quarantinePaths.slice(0,3); track path){<small>{{ basename(path) }}</small>}</div><b>{{ formatBytes(group.reclaimableBytes) }}</b></article>
                    } @empty { <div class="preview-empty">Aucun doublon strictement identique détecté.</div> }
                  </div>
                  @if(plan.warnings.length){<div class="warning-box">{{ plan.warnings.length }} fichier(s) n’ont pas pu être vérifiés. Ils ne seront pas touchés.</div>}
                  <footer class="apply-bar"><span>{{ plan.quarantineCount }} copie(s) seront mises à l’écart.</span><button class="ff-button" type="button" [disabled]="busy() || plan.quarantineCount===0" (click)="quarantineDuplicates()">Mettre les copies en quarantaine</button></footer>
                } @else { <div class="preview-placeholder"><span>≡</span><strong>FileFlow confirme les doublons octet par octet.</strong><p>Taille → empreinte partielle → SHA-256 complet seulement pour les candidats crédibles.</p></div> }
              </section>
            </section>
          }
        }
      }
    </div>
  `,
  styles: [`
    :host{display:block}.organize-shell{max-width:1180px;margin:0 auto}.organize-head{display:flex;align-items:flex-end;justify-content:space-between;gap:28px}.organize-head>div{max-width:820px}.organize-head h1{margin:0;font-size:46px;letter-spacing:-.05em;line-height:1.02}.organize-head p:last-child{max-width:760px;margin:10px 0 0;color:var(--text-muted);font-size:12px;line-height:1.6}.workspace-pill{padding:8px 11px;border:1px solid var(--border);border-radius:999px;background:var(--surface-2);color:var(--text-muted);font-size:10px;font-weight:800}.mode-tabs{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin:25px 0 12px}.mode-tabs button{display:grid;grid-template-columns:38px 1fr;grid-template-rows:auto auto;gap:2px 8px;min-height:68px;padding:12px;border:1px solid var(--border);border-radius:13px;background:var(--surface-1);color:var(--text-muted);text-align:left}.mode-tabs button>span{grid-row:1/3;width:34px;height:34px;display:grid;place-items:center;border-radius:9px;background:var(--surface-2);font-size:10px;font-weight:900}.mode-tabs strong{color:var(--text);font-size:12px}.mode-tabs small{font-size:10px;color:var(--text-faint)}.mode-tabs button.active{border-color:color-mix(in srgb,var(--accent) 45%,var(--border));background:var(--accent-soft)}.mode-tabs button.active>span{background:var(--accent);color:white}.tool-grid{display:grid;grid-template-columns:330px minmax(0,1fr);gap:10px}.controls,.preview{min-height:490px;padding:16px}.section-title span,.preview-head span{display:block;color:var(--text-faint);font-size:9px;font-weight:900;letter-spacing:.08em}.section-title strong,.preview-head strong{display:block;margin-top:4px;font-size:15px}.controls label{display:grid;gap:5px;margin-top:14px;color:var(--text-muted);font-size:10px;font-weight:750}.controls input,.controls select{width:100%;height:35px;padding:0 9px;border:1px solid var(--border);border-radius:9px;background:var(--bg-elevated);color:var(--text);font:inherit;font-size:11px}.controls .ff-button{width:100%;margin-top:16px}.two-col{display:grid;grid-template-columns:1fr 1fr;gap:8px}.token-row{display:flex;flex-wrap:wrap;gap:5px;margin-top:8px}.token-row button{padding:5px 7px;border:1px solid var(--border);border-radius:7px;background:var(--surface-2);color:var(--text-muted);font-family:ui-monospace,SFMono-Regular,monospace;font-size:9px}.check{display:flex!important;align-items:center;grid-template-columns:auto 1fr!important}.check input{width:15px;height:15px}.folder-field{display:grid;grid-template-columns:1fr auto;gap:5px}.folder-field button{border:1px solid var(--border);border-radius:9px;background:var(--surface-2);color:var(--accent);font-size:10px;font-weight:800;padding:0 9px}.explanation,.safety-note{display:grid;gap:5px;margin-top:15px;padding:11px;border:1px solid var(--border);border-radius:10px;background:var(--surface-2);color:var(--text-muted);font-size:10px;line-height:1.45}.explanation code{overflow:hidden;text-overflow:ellipsis;color:var(--accent);font-size:9px}.safety-note b{color:var(--text)}.preview{overflow:hidden}.preview-head{display:flex;align-items:center;justify-content:space-between;padding-bottom:12px;border-bottom:1px solid var(--border)}.summary-line{display:flex;justify-content:space-between;gap:12px;padding:9px 0;color:var(--text-muted);font-size:10px}.summary-line small{color:var(--text-faint)}.rename-list,.duplicate-list{max-height:380px;overflow:auto;overscroll-behavior:contain}.rename-list article{display:grid;grid-template-columns:minmax(0,1fr) 26px minmax(0,1fr);align-items:center;gap:7px;min-height:42px;padding:6px 4px;border-bottom:1px solid var(--border);font-size:10px}.rename-list article i{text-align:center;color:var(--text-faint);font-style:normal}.rename-list article small{grid-column:1/-1;color:var(--warning)}.rename-list .before,.rename-list .after{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.rename-list .after{color:var(--accent);font-weight:750}.rename-list .after.path{font-size:9px}.rename-list article.conflict{background:var(--danger-soft)}.preview-placeholder{min-height:390px;display:grid;place-items:center;align-content:center;text-align:center}.preview-placeholder>span{width:58px;height:58px;display:grid;place-items:center;border-radius:16px;background:var(--accent-soft);color:var(--accent);font-size:20px}.preview-placeholder strong{margin-top:12px;font-size:13px}.preview-placeholder p{max-width:430px;margin:6px 0;color:var(--text-muted);font-size:10px;line-height:1.5}.preview-empty{padding:32px;text-align:center;color:var(--text-muted);font-size:11px}.apply-bar{display:flex;align-items:center;justify-content:space-between;gap:15px;margin-top:12px;padding-top:12px;border-top:1px solid var(--border)}.apply-bar span{color:var(--text-faint);font-size:9px}.danger-pill,.success-pill{padding:5px 8px;border-radius:999px;font-size:9px}.danger-pill{background:var(--danger-soft);color:var(--danger)}.success-pill{background:var(--success-soft);color:var(--success)}.category-chips{display:flex;flex-wrap:wrap;gap:5px;padding:10px 0}.category-chips span{display:flex;gap:5px;padding:5px 7px;border-radius:999px;background:var(--surface-2);color:var(--text-muted);font-size:9px}.category-chips b{color:var(--text)}.duplicate-list article{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr) auto;align-items:center;gap:12px;padding:10px 4px;border-bottom:1px solid var(--border)}.duplicate-list article span,.duplicate-list article strong,.duplicate-list article small{display:block}.duplicate-list article span{color:var(--text-faint);font-size:8px;text-transform:uppercase;font-weight:850}.duplicate-list article strong{margin-top:2px;font-size:10px}.duplicate-list article small{margin-top:2px;color:var(--text-muted);font-size:9px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.duplicate-list article>b{color:var(--success);font-size:10px}.warning-box{margin-top:9px;padding:8px;border-radius:8px;background:var(--warning-soft);color:var(--warning);font-size:9px}.message{margin:9px 0;padding:9px 11px;border-radius:9px;font-size:10px}.message.error{background:var(--danger-soft);color:var(--danger)}.message.success{background:var(--success-soft);color:var(--success)}.empty{min-height:420px;margin-top:25px;display:grid;place-items:center;align-content:center;text-align:center;padding:35px}.empty-mark{width:62px;height:62px;display:grid;place-items:center;border-radius:17px;background:var(--accent-soft);color:var(--accent);font-size:22px}.empty h2{margin:14px 0 0}.empty p{max-width:500px;color:var(--text-muted);font-size:11px}.empty .ff-button{margin-top:10px}@media(max-width:850px){.organize-head{display:block}.organize-head h1{font-size:38px}.workspace-pill{display:inline-flex;margin-top:10px}.tool-grid{grid-template-columns:1fr}.controls,.preview{min-height:0}.mode-tabs{grid-template-columns:1fr 1fr 1fr}}@media(max-width:600px){.mode-tabs{grid-template-columns:1fr}.organize-head h1{font-size:32px}.two-col{grid-template-columns:1fr}.apply-bar{align-items:stretch;flex-direction:column}.duplicate-list article{grid-template-columns:1fr}.rename-list article{grid-template-columns:minmax(0,1fr) 20px minmax(0,1fr)}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class OrganizePage {
  private readonly bridge = inject(TauriBridgeService);
  private readonly router = inject(Router);
  protected readonly workspace = inject(WorkspaceStore);
  protected readonly mode = signal<OrganizeMode>('rename');
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly notice = signal<string | null>(null);
  protected readonly renamePreview = signal<RenamePreview | null>(null);
  protected readonly organizationPreview = signal<OrganizationPreview | null>(null);
  protected readonly duplicatePlan = signal<DuplicateCleanupPlan | null>(null);
  protected readonly destination = signal('');
  protected readonly organizationMode = signal<OrganizationMode>('typeDate');
  protected readonly duplicateStrategy = signal<DuplicateStrategy>('newest');
  protected readonly renameRule = signal<RenameRule>({ template: '{date}-{name}-{counter}', search: '', replace: '', counterStart: 1, counterPadding: 3, caseMode: 'keep', preserveExtension: true });
  protected readonly renameTokens = ['{name}','{counter}','{date}','{year}','{month}','{day}','{parent}','{ext}'];
  protected readonly selectedIds = computed(() => [...this.workspace.selectedIds()]);

  protected selectMode(mode: OrganizeMode): void { this.mode.set(mode); this.error.set(null); this.notice.set(null); }
  protected targetCountLabel(): string { const selected=this.workspace.selectedCount(); return selected ? `${selected} sélectionné(s)` : `${this.workspace.counts().files + this.workspace.counts().archives} élément(s)`; }
  protected goWorkspace(): void { void this.router.navigate(['/workspace']); }

  protected updateRename(key: keyof RenameRule, value: string): void { this.renameRule.update((rule) => ({...rule,[key]:value})); this.renamePreview.set(null); }
  protected updateRenameNumber(key: 'counterStart'|'counterPadding', value: string): void { const parsed=Number(value); if(Number.isFinite(parsed)) this.renameRule.update((rule)=>({...rule,[key]:Math.max(key==='counterPadding'?1:0,Math.floor(parsed))})); this.renamePreview.set(null); }
  protected updateRenameBool(key: 'preserveExtension', value: boolean): void { this.renameRule.update((rule)=>({...rule,[key]:value})); this.renamePreview.set(null); }
  protected insertToken(token: string): void { this.renameRule.update((rule)=>({...rule,template:`${rule.template}${token}`})); this.renamePreview.set(null); }

  protected async previewRename(): Promise<void> { const id=this.workspace.workspace()?.id; if(!id)return; await this.guard(async()=>{ this.renamePreview.set(await this.bridge.previewBatchRename(id,this.selectedIds(),this.renameRule())); }); }
  protected async applyRename(): Promise<void> { const id=this.workspace.workspace()?.id; const preview=this.renamePreview(); if(!id||!preview||preview.conflicts)return; const roots=[...(this.workspace.workspace()?.roots??[])]; await this.guard(async()=>{ const result=await this.bridge.applyBatchRename(id,this.selectedIds(),this.renameRule()); this.notice.set(`${result.processed} élément(s) renommé(s).`); this.renamePreview.set(null); if(roots.length) await this.workspace.start(roots); }); }

  protected async chooseDestination(): Promise<void> { const selected=await this.bridge.chooseStorageDirectory(); if(selected){this.destination.set(selected);this.organizationPreview.set(null);} }
  protected organizationExample(): string { return this.organizationMode()==='type' ? 'FileFlow/Images · PDF · Vidéos…' : this.organizationMode()==='date' ? 'FileFlow/2026/08/…' : 'FileFlow/Images/2026/08/…'; }
  protected async previewOrganization(): Promise<void> { const id=this.workspace.workspace()?.id; if(!id||!this.destination())return; await this.guard(async()=>{ this.organizationPreview.set(await this.bridge.previewOrganization(id,this.selectedIds(),this.destination(),this.organizationMode())); }); }
  protected async applyOrganization(): Promise<void> { const id=this.workspace.workspace()?.id; const preview=this.organizationPreview(); if(!id||!preview||!this.destination())return; const destination=this.destination(); await this.guard(async()=>{ const result=await this.bridge.applyOrganization(id,this.selectedIds(),destination,this.organizationMode()); this.notice.set(`${result.processed} fichier(s) classé(s).`); this.organizationPreview.set(null); await this.workspace.start([destination]); }); }

  protected async scanDuplicates(): Promise<void> { const id=this.workspace.workspace()?.id; if(!id)return; await this.guard(async()=>{ this.duplicatePlan.set(await this.bridge.duplicateCleanupPlan(id,this.selectedIds(),this.duplicateStrategy())); }); }
  protected async quarantineDuplicates(): Promise<void> { const id=this.workspace.workspace()?.id; const plan=this.duplicatePlan(); if(!id||!plan)return; const ids=plan.groups.flatMap((group)=>group.quarantineAssetIds); const roots=[...(this.workspace.workspace()?.roots??[])]; await this.guard(async()=>{ const result=await this.bridge.quarantineDuplicates(id,ids,null); this.notice.set(`${result.processed} copie(s) déplacée(s) vers ${result.destination ?? 'la quarantaine'}.`); this.duplicatePlan.set(null); if(roots.length) await this.workspace.start(roots); }); }

  protected categoryEntries(preview: OrganizationPreview): [string,number][] { return Object.entries(preview.categories).sort((a,b)=>b[1]-a[1]); }
  protected basename(path: string): string { return path.replace(/\\/g,'/').split('/').pop() || path; }
  protected shortPath(path: string): string { const parts=path.replace(/\\/g,'/').split('/').filter(Boolean); return parts.slice(-3).join('/'); }
  protected shortTarget(path: string): string { return this.shortPath(path); }
  protected formatBytes(bytes: number): string { if(bytes<1024)return`${bytes} o`;const u=['Ko','Mo','Go','To'];let v=bytes/1024,i=0;while(i<u.length-1&&v>=1024){v/=1024;i++}return`${v>=10?v.toFixed(0):v.toFixed(1)} ${u[i]}`; }

  private async guard(task:()=>Promise<void>): Promise<void> { if(this.busy())return; this.busy.set(true); this.error.set(null); this.notice.set(null); try{await task();}catch(error){this.error.set(message(error));}finally{this.busy.set(false);} }
}

function message(error: unknown): string { return error instanceof Error ? error.message : typeof error === 'string' ? error : 'L’opération n’a pas pu être préparée.'; }
