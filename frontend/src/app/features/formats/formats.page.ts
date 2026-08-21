import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor, FormatCapabilityProfile } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

@Component({
  selector: 'ff-formats-page',
  template: `
    <div class="formats-shell">
      <header class="formats-head">
        <div><p class="ff-kicker">FORMATS & POSSIBILITÉS</p><h1>Tout ce que FileFlow sait faire, format par format.</h1><p>« Reconnu » ne veut pas dire « convertible ». Cette matrice distingue lecture, écriture, aperçu, métadonnées, extraction, transformations et actions réellement exécutables sur cette machine.</p></div>
        <div class="catalog-stat"><strong>{{ filtered().length }}</strong><span>profils de format</span></div>
      </header>

      <section class="format-toolbar ff-card">
        <label><span>⌕</span><input #search type="search" placeholder="JPEG, RAW, PDF, MKV, EPUB, ZIP…" [value]="query()" (input)="query.set(search.value)" /></label>
        <select [value]="family()" (change)="family.set($any($event.target).value)"><option value="all">Toutes les familles</option>@for(entry of families(); track entry){<option [value]="entry">{{ familyLabel(entry) }}</option>}</select>
        <label class="ready-only"><input type="checkbox" [checked]="readyOnly()" (change)="readyOnly.set($any($event.target).checked)" /><span>Actions exécutables uniquement</span></label>
      </section>

      <div class="format-grid">
        @for (format of filtered(); track format.id) {
          <article class="format-card ff-card">
            <header><div class="format-mark">{{ format.id.slice(0,4).toUpperCase() }}</div><div><strong>{{ format.label }}</strong><span>{{ familyLabel(format.family) }}</span></div><b>{{ format.extensions.length }} ext.</b></header>
            <div class="extensions">@for(ext of format.extensions.slice(0,12); track ext){<code>.{{ ext }}</code>}@if(format.extensions.length>12){<code>+{{ format.extensions.length-12 }}</code>}</div>
            <div class="capability-row">
              @for (item of capabilityLabels(format); track item.label) { <span [class.on]="item.on">{{ item.on ? '✓' : '—' }} {{ item.label }}</span> }
            </div>
            @if(format.capabilities.length){<div class="technical-tags">@for(cap of format.capabilities; track cap){<span>{{ capabilityName(cap) }}</span>}</div>}
            <section class="action-list">
              <div class="subhead"><strong>Actions</strong><span>{{ actionCount(format) }} disponible(s)</span></div>
              @for(action of actionsFor(format).slice(0,10); track action.id){
                <button type="button" [class.not-ready]="!capabilities.isActionExecutable(action)" (click)="openAction(action)"><div><strong>{{ action.title }}</strong><small>{{ action.description }}</small></div><span>{{ capabilities.isActionExecutable(action) ? 'Ouvrir' : actionState(action) }}</span></button>
              } @empty { <p class="no-actions">Aucune transformation locale exécutable pour ce profil.</p> }
            </section>
            <footer>
              @if(format.convertTo.length){<div><span>Convertir vers</span><p>@for(target of format.convertTo; track target){<code>{{ target.toUpperCase() }}</code>}</p></div>}
              @if(format.compressTo.length){<div><span>Compresser vers</span><p>@for(target of format.compressTo.slice(0,10); track target){<code>{{ target.toUpperCase() }}</code>}</p></div>}
            </footer>
          </article>
        } @empty {
          <section class="empty ff-card"><strong>Aucun format ne correspond.</strong><span>Essayez une extension comme « heic », « pdf », « mkv » ou « zst ».</span></section>
        }
      </div>
    </div>
  `,
  styles: [`
    :host{display:block}.formats-shell{max-width:1220px;margin:0 auto}.formats-head{display:grid;grid-template-columns:minmax(0,1fr) 132px;gap:28px;align-items:end}.formats-head>div:first-child{max-width:900px}.formats-head h1{margin:0;font-size:clamp(38px,4.6vw,58px);letter-spacing:-.055em;line-height:1}.formats-head p:last-child{margin:13px 0 0;color:var(--text-muted);font-size:14px;line-height:1.65}.catalog-stat{padding:17px;border:1px solid var(--border);border-radius:18px;background:linear-gradient(145deg,var(--surface-1),var(--accent-soft-2));box-shadow:var(--shadow-sm);text-align:center}.catalog-stat strong,.catalog-stat span{display:block}.catalog-stat strong{font-size:31px}.catalog-stat span{margin-top:2px;color:var(--text-faint);font-size:10px;text-transform:uppercase;font-weight:800}.format-toolbar{display:grid;grid-template-columns:minmax(280px,1fr) 210px auto;gap:9px;align-items:center;margin:28px 0 12px;padding:10px;border-radius:16px}.format-toolbar>label:first-child{display:grid;grid-template-columns:23px 1fr;align-items:center;padding:0 9px}.format-toolbar input[type=search],.format-toolbar select{height:40px;border:0!important;background:transparent!important;box-shadow:none!important;color:var(--text);outline:0;font-size:13px}.format-toolbar select{padding:0 9px;border-left:1px solid var(--border)!important}.ready-only{display:flex;align-items:center;gap:7px;color:var(--text-muted);font-size:11.5px}.ready-only input{width:17px;height:17px;accent-color:var(--accent)}
    .format-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:11px}.format-card{padding:18px;min-width:0;border-radius:19px}.format-card>header{display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:11px}.format-mark{width:46px;height:46px;display:grid;place-items:center;border-radius:14px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:900}.format-card header strong,.format-card header span{display:block}.format-card header strong{font-size:15px}.format-card header span{margin-top:3px;color:var(--text-muted);font-size:11px}.format-card header>b{color:var(--text-faint);font-size:9px}.extensions,.technical-tags{display:flex;flex-wrap:wrap;gap:5px;margin-top:12px}.extensions code,.format-card footer code{padding:4px 6px;border-radius:7px;background:var(--surface-2);color:var(--text-muted);font-size:9.5px}.capability-row{display:flex;flex-wrap:wrap;gap:5px;margin-top:11px}.capability-row span{padding:5px 7px;border-radius:8px;background:var(--surface-2);color:var(--text-faint);font-size:9.5px}.capability-row span.on{background:var(--success-soft);color:var(--success)}.technical-tags span{padding:4px 7px;border:1px solid color-mix(in srgb,var(--accent) 20%,var(--border));border-radius:999px;color:var(--accent);font-size:9.5px}.action-list{margin-top:15px;border-top:1px solid var(--border);padding-top:12px}.subhead{display:flex;justify-content:space-between;align-items:center;margin-bottom:6px}.subhead strong{font-size:10px;text-transform:uppercase;letter-spacing:.08em}.subhead span{color:var(--text-faint);font-size:9.5px}.action-list button{width:100%;display:grid;grid-template-columns:1fr auto;gap:10px;align-items:center;padding:9px 6px;border:0;border-bottom:1px solid var(--border);background:transparent;color:var(--text);text-align:left}.action-list button:hover{background:var(--surface-2)}.action-list button strong,.action-list button small{display:block}.action-list button strong{font-size:11.5px}.action-list button small{margin-top:3px;color:var(--text-faint);font-size:10.5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.action-list button>span{color:var(--accent);font-size:9.5px;font-weight:800}.action-list button.not-ready{opacity:.6}.action-list button.not-ready>span{color:var(--warning)}.no-actions{color:var(--text-faint);font-size:10.5px}.format-card>footer{display:grid;grid-template-columns:1fr 1fr;gap:8px;margin-top:12px}.format-card footer>div{padding:10px;border-radius:10px;background:var(--surface-2)}.format-card footer span{display:block;color:var(--text-faint);font-size:9px;font-weight:850;text-transform:uppercase}.format-card footer p{display:flex;flex-wrap:wrap;gap:4px;margin:6px 0 0}.empty{grid-column:1/-1;min-height:240px;display:grid;place-items:center;align-content:center;text-align:center}.empty span{margin-top:6px;color:var(--text-muted);font-size:12px}@media(max-width:900px){.format-grid{grid-template-columns:1fr}.format-toolbar{grid-template-columns:1fr 180px}.ready-only{grid-column:1/-1}}@media(max-width:600px){.formats-head{grid-template-columns:1fr}.catalog-stat{width:max-content}.format-toolbar{grid-template-columns:1fr}.format-toolbar select{border-left:0!important;border-top:1px solid var(--border)!important}.format-card>footer{grid-template-columns:1fr}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class FormatsPage {
  protected readonly capabilities = inject(CapabilityStore);
  private readonly workspace = inject(WorkspaceStore);
  private readonly router = inject(Router);
  protected readonly query = signal('');
  protected readonly family = signal('all');
  protected readonly readyOnly = signal(false);
  protected readonly families = computed(() => [...new Set(this.capabilities.formats().map((format)=>format.family))].sort());
  protected readonly filtered = computed(() => {
    const query=this.query().trim().toLowerCase(); const family=this.family(); const readyOnly=this.readyOnly();
    return this.capabilities.formats().filter((format)=>{
      if(family!=='all'&&format.family!==family)return false;
      if(query&&!`${format.id} ${format.label} ${format.extensions.join(' ')} ${format.capabilities.join(' ')}`.toLowerCase().includes(query))return false;
      if(readyOnly&&!this.actionsFor(format).some((action)=>this.capabilities.isActionExecutable(action)))return false;
      return true;
    });
  });
  protected actionsFor(format: FormatCapabilityProfile): ActionDescriptor[] { return format.actions.map((id)=>this.capabilities.action(id)).filter((action):action is ActionDescriptor=>!!action); }
  protected actionCount(format: FormatCapabilityProfile): number { return this.actionsFor(format).filter((action)=>this.capabilities.isActionExecutable(action)).length; }
  protected actionState(action: ActionDescriptor): string { return this.capabilities.actionState(action)==='missing-engine' ? 'Moteur absent' : 'Prévu'; }
  protected openAction(action: ActionDescriptor): void { if(!this.capabilities.isActionExecutable(action))return; this.workspace.setPendingAction(action.id); if(this.workspace.hasWorkspace()){this.workspace.openAction(action.id);void this.router.navigate(['/workspace']);}else{void this.router.navigate(['/']);} }
  protected capabilityLabels(format: FormatCapabilityProfile): {label:string;on:boolean}[] { return [{label:'Aperçu',on:format.preview},{label:'Lecture',on:format.readable},{label:'Écriture',on:format.writable},{label:'Métadonnées',on:format.metadata},{label:'Miniature',on:format.thumbnail},{label:'Extraction',on:format.extractable},{label:'Streaming',on:format.streamable}]; }
  protected capabilityName(value:string):string{return CAPABILITY_LABELS[value]??value;}
  protected familyLabel(value:string):string{return FAMILY_LABELS[value]??value;}
}

const CAPABILITY_LABELS:Record<string,string>={inspect:'Inspecter',preview:'Prévisualiser',convert:'Convertir',compress:'Compresser',metadata:'Métadonnées',thumbnail:'Miniatures',extract:'Extraire',stream:'Lire en flux',editPixels:'Éditer les pixels',batch:'Traitement par lot',privacy:'Confidentialité',pages:'Pages',ocr:'OCR',repair:'Réparer',transcode:'Transcoder',documentTransform:'Transformer le document'};
const FAMILY_LABELS:Record<string,string>={image:'Images',pdf:'PDF',document:'Documents',spreadsheet:'Tableurs',presentation:'Présentations',text:'Texte & données',ebook:'Livres numériques',audio:'Audio',video:'Vidéo',archive:'Archives',unknown:'Autres'};
