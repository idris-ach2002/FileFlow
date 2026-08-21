import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { open } from '@tauri-apps/plugin-dialog';
import { ActionDescriptor, RecipeRecord, WorkflowStep } from '../../core/ipc/tauri.models';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';
import { AutomationStore } from './automation.store';

interface TemplateStep {
  actionId: string;
  label: string;
  targetFormat?: string;
  quality?: string;
  parameters?: Record<string, string | number | boolean | null>;
}
interface RecipeTemplate {
  id: string;
  icon: string;
  name: string;
  description: string;
  trigger: string;
  steps: TemplateStep[];
}

@Component({
  selector: 'ff-automations-page',
  imports: [DatePipe],
  template: `
    <div class="automation-shell">
      <header class="page-head">
        <div><p class="ff-kicker">AUTOMATISER</p><h1>Une fois configuré, FileFlow s’en charge.</h1><p>Créez une recette simple, lancez-la sur le workspace courant ou associez-la à un dossier surveillé. Chaque étape est checkpointée pour pouvoir reprendre après une interruption.</p></div>
        <button class="ff-button" type="button" (click)="builderOpen.set(true)">＋ Nouvelle recette</button>
      </header>

      <section class="status-grid">
        <article class="ff-card"><span>Recettes</span><strong>{{ store.recipes().length }}</strong><small>enregistrées localement</small></article>
        <article class="ff-card"><span>Dossiers surveillés</span><strong>{{ store.watchedFolders().length }}</strong><small>actifs uniquement si vous le décidez</small></article>
        <article class="ff-card"><span>À reprendre</span><strong>{{ store.recoverableJobs().length }}</strong><small>jobs interrompus/échoués</small></article>
      </section>

      @if (store.workflowEvent(); as progress) {
        <section class="progress-card ff-card">
          <div><strong>{{ progress.message || 'Workflow en cours' }}</strong><small>Étape {{ progress.completedSteps }} / {{ progress.totalSteps }}</small></div>
          <div class="progress-track"><i [style.width.%]="progress.totalSteps ? (progress.completedSteps / progress.totalSteps) * 100 : 0"></i></div>
          @if (store.runningJobId()) { <button class="ff-button secondary" type="button" (click)="store.cancel()">Annuler</button> }
        </section>
      }
      @if (store.error()) { <p class="error-banner">{{ store.error() }}</p> }
      @if (store.notice()) { <p class="notice-banner">{{ store.notice() }}</p> }

      <div class="section-title"><div><p class="ff-kicker">DÉMARRER FACILEMENT</p><h2>Modèles prêts à adapter</h2></div><span>Pas de graphe technique à comprendre.</span></div>
      <section class="template-grid">
        @for (recipe of templates; track recipe.id) {
          <article class="template-card ff-card">
            <div class="template-top"><span>{{ recipe.icon }}</span><em>{{ recipe.trigger }}</em></div>
            <h3>{{ recipe.name }}</h3><p>{{ recipe.description }}</p>
            <ol>@for (step of recipe.steps; track step.actionId) { <li><i></i>{{ step.label }}</li> }</ol>
            <button class="ff-button secondary" type="button" (click)="saveTemplate(recipe)">Enregistrer ce modèle</button>
          </article>
        }
      </section>

      <div class="section-title"><div><p class="ff-kicker">MES RECETTES</p><h2>Lancer sur les fichiers ouverts</h2></div><span>Si rien n’est sélectionné, tous les fichiers compatibles du workspace sont utilisés.</span></div>
      <section class="recipes ff-card">
        @for (recipe of store.recipes(); track recipe.id) {
          <article>
            <span class="recipe-icon">{{ recipe.icon }}</span>
            <div><strong>{{ recipe.name }}</strong><small>{{ recipe.description }}</small></div>
            <span class="state" [class.off]="!recipe.enabled">{{ recipe.enabled ? 'Active' : 'Pause' }}</span>
            <button class="ff-button secondary" type="button" [disabled]="!canRunWorkspace() || !recipe.enabled || !!store.runningJobId()" (click)="runOnWorkspace(recipe)">Lancer</button>
          </article>
        } @empty { <div class="empty">Enregistrez un modèle ci-dessus ou créez votre propre recette.</div> }
      </section>

      <div class="two-columns">
        <section>
          <div class="section-title"><div><p class="ff-kicker">DOSSIERS SURVEILLÉS</p><h2>Quand un fichier arrive…</h2></div><button class="mini-button" type="button" (click)="watchOpen.set(true)">＋ Ajouter</button></div>
          <div class="watch-list ff-card">
            @for (watch of store.watchedFolders(); track watch.id) {
              <article><span>📁</span><div><strong>{{ shortPath(watch.path) }}</strong><small>{{ recipeName(watch.recipeId) }} · {{ watch.recursive ? 'avec sous-dossiers' : 'ce dossier' }} · stabilité {{ watch.stabilitySeconds }} s</small></div><em>{{ watch.enabled ? 'Surveillé' : 'Pause' }}</em><button type="button" (click)="store.deleteWatch(watch.id)">×</button></article>
            } @empty { <div class="empty">Aucune surveillance. FileFlow ne traite jamais un dossier en arrière-plan sans votre configuration explicite.</div> }
          </div>
        </section>

        <section>
          <div class="section-title"><div><p class="ff-kicker">REPRISE</p><h2>Après fermeture ou incident</h2></div></div>
          <div class="job-list ff-card">
            @for (job of store.recoverableJobs().slice(0, 8); track job.id) {
              <article><span class="job-state">{{ job.status === 'interrupted' ? '!' : '↺' }}</span><div><strong>{{ recipeName(job.recipeId) }}</strong><small>{{ job.currentStep }}/{{ job.totalSteps }} étape(s) terminée(s) · {{ job.updatedAt | date:'short' }}</small></div><button class="mini-button" type="button" [disabled]="!!store.runningJobId()" (click)="store.resume(job.id)">Reprendre</button></article>
            } @empty { <div class="empty">Aucun job à reprendre.</div> }
          </div>
        </section>
      </div>

      @if (watchOpen()) {
        <div class="modal-backdrop" (click)="watchOpen.set(false)"><section class="modal ff-card" (click)="$event.stopPropagation()">
          <button class="close" type="button" (click)="watchOpen.set(false)">×</button><p class="ff-kicker">DOSSIER SURVEILLÉ</p><h2>Que doit faire FileFlow ?</h2>
          <label>Dossier<button class="picker" type="button" (click)="chooseWatchFolder()">{{ watchPath() || 'Choisir un dossier…' }}</button></label>
          <label>Recette<select #watchRecipe (change)="watchRecipeId.set(watchRecipe.value)"><option value="">Choisir…</option>@for (recipe of store.recipes(); track recipe.id) {<option [value]="recipe.id">{{ recipe.name }}</option>}</select></label>
          <label>Extensions (facultatif)<input #extensions value="" placeholder="pdf, jpg, png" (input)="watchExtensions.set(extensions.value)" /></label>
          <label class="toggle"><input #recursive type="checkbox" (change)="watchRecursive.set(recursive.checked)" /><span>Inclure les sous-dossiers</span></label>
          <label>Attendre qu’un fichier soit stable <div class="inline"><input #stability type="number" min="1" max="300" value="3" (input)="watchStability.set(+stability.value || 3)"/><span>secondes</span></div></label>
          <div class="modal-actions"><button class="ff-button secondary" type="button" (click)="watchOpen.set(false)">Annuler</button><button class="ff-button" type="button" [disabled]="!watchPath() || !watchRecipeId()" (click)="saveWatch()">Activer la surveillance</button></div>
        </section></div>
      }

      @if (builderOpen()) {
        <div class="modal-backdrop" (click)="closeBuilder()"><section class="modal recipe-builder ff-card" (click)="$event.stopPropagation()">
          <button class="close" type="button" (click)="closeBuilder()">×</button><p class="ff-kicker">RECETTE PERSONNALISÉE</p><h2>Décrivez le résultat, pas la technique.</h2><p class="modal-lead">Ajoutez les opérations dans l’ordre. FileFlow construit un workflow DAG, transmet les sorties à l’étape suivante et sauvegarde un checkpoint après chaque étape.</p>
          <div class="builder-fields"><label>Nom<input #recipeName maxlength="64" [value]="builderName()" (input)="builderName.set(recipeName.value)" placeholder="Ex. Photos pour mon site" /></label><label>Description<input #recipeDescription maxlength="160" [value]="builderDescription()" (input)="builderDescription.set(recipeDescription.value)" placeholder="Ce que cette recette prépare" /></label></div>
          <div class="add-step"><select #stepSelect><option value="">Ajouter une action…</option>@for(action of executableBuilderActions();track action.id){<option [value]="action.id">{{ action.title }}</option>}</select><button class="mini-button" type="button" [disabled]="!stepSelect.value" (click)="addBuilderStep(stepSelect.value);stepSelect.value=''">Ajouter</button></div>
          <div class="builder-steps">
            @for(actionId of builderSteps();track $index;let i=$index){@if(actionById(actionId);as action){<article><span>{{ i+1 }}</span><div><strong>{{ action.title }}</strong><small>{{ action.description }}</small></div><div class="step-buttons"><button type="button" [disabled]="i===0" (click)="moveBuilderStep(i,-1)">↑</button><button type="button" [disabled]="i===builderSteps().length-1" (click)="moveBuilderStep(i,1)">↓</button><button type="button" (click)="removeBuilderStep(i)">×</button></div></article>}}
            @empty { <div class="builder-empty">Ajoutez une première action. Vous pourrez tester la recette sur quelques fichiers avant d’activer une surveillance.</div> }
          </div>
          <div class="builder-note"><b>Protection intégrée</b><span>Originaux conservés · noms de sortie incrémentés · cancellation · reprise après crash.</span></div>
          <div class="modal-actions"><button class="ff-button secondary" type="button" (click)="closeBuilder()">Annuler</button><button class="ff-button" type="button" [disabled]="!builderName().trim() || builderSteps().length===0" (click)="saveCustomRecipe()">Enregistrer la recette</button></div>
        </section></div>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.automation-shell{max-width:1240px;margin:0 auto}.page-head{display:flex;justify-content:space-between;align-items:end;gap:24px}.page-head>div{max-width:820px}.page-head h1{margin:0;font-size:clamp(38px,5vw,58px);line-height:.98;letter-spacing:-.055em}.page-head p:last-child{max-width:760px;margin:13px 0 0;color:var(--text-muted);font-size:13px;line-height:1.65}.status-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-top:28px}.status-grid article{padding:15px}.status-grid span,.status-grid small{display:block;color:var(--text-muted);font-size:10px}.status-grid strong{display:block;margin:7px 0 3px;font-size:28px;letter-spacing:-.04em}.section-title{display:flex;align-items:end;justify-content:space-between;gap:16px;margin:34px 0 11px}.section-title h2{margin:0;font-size:23px;letter-spacing:-.035em}.section-title>span{color:var(--text-faint);font-size:10px}.template-grid{display:grid;grid-template-columns:repeat(4,1fr);gap:10px}.template-card{min-height:306px;padding:16px;display:flex;flex-direction:column}.template-top{display:flex;justify-content:space-between}.template-top>span{width:40px;height:40px;display:grid;place-items:center;border-radius:11px;background:var(--accent-soft);font-size:18px}.template-top em{height:24px;padding:0 8px;display:grid;place-items:center;border-radius:999px;background:var(--surface-2);color:var(--text-muted);font-size:9px;font-style:normal}.template-card h3{margin:18px 0 6px;font-size:16px}.template-card p{margin:0;color:var(--text-muted);font-size:10px;line-height:1.5}.template-card ol{display:grid;gap:7px;padding:0;margin:15px 0;list-style:none}.template-card li{display:flex;gap:7px;align-items:center;color:var(--text-muted);font-size:10px}.template-card li i{width:5px;height:5px;border-radius:50%;background:var(--accent)}.template-card button{margin-top:auto}.recipes,.watch-list,.job-list{overflow:hidden}.recipes article{display:grid;grid-template-columns:38px minmax(0,1fr) auto auto;align-items:center;gap:10px;padding:11px 13px;border-bottom:1px solid var(--border)}.recipes article:last-child{border-bottom:0}.recipe-icon{width:34px;height:34px;display:grid;place-items:center;border-radius:10px;background:var(--accent-soft)}.recipes strong,.recipes small,.watch-list strong,.watch-list small,.job-list strong,.job-list small{display:block}.recipes strong,.watch-list strong,.job-list strong{font-size:11px}.recipes small,.watch-list small,.job-list small{margin-top:3px;color:var(--text-muted);font-size:9px}.state{color:var(--success);font-size:9px}.state.off{color:var(--text-faint)}.recipes button{min-height:30px}.two-columns{display:grid;grid-template-columns:1fr 1fr;gap:14px}.watch-list article,.job-list article{display:grid;grid-template-columns:32px minmax(0,1fr) auto auto;align-items:center;gap:9px;padding:10px 12px;border-bottom:1px solid var(--border)}.watch-list article:last-child,.job-list article:last-child{border-bottom:0}.watch-list em{color:var(--success);font-size:9px;font-style:normal}.watch-list article>button{width:26px;height:26px;border:0;border-radius:7px;background:var(--surface-2);color:var(--text-muted)}.job-state{width:28px;height:28px;display:grid;place-items:center;border-radius:9px;background:color-mix(in srgb,var(--warning) 14%,transparent);color:var(--warning);font-weight:900}.mini-button{min-height:29px;padding:0 9px;border:1px solid var(--border);border-radius:8px;background:var(--surface-2);color:var(--text);font-size:9px;font-weight:750}.empty{padding:22px;color:var(--text-muted);font-size:10px;text-align:center}.progress-card{display:grid;grid-template-columns:minmax(190px,auto) minmax(180px,1fr) auto;align-items:center;gap:16px;margin-top:14px;padding:12px 14px}.progress-card strong,.progress-card small{display:block}.progress-card strong{font-size:11px}.progress-card small{margin-top:3px;color:var(--text-muted);font-size:9px}.progress-track{height:6px;overflow:hidden;border-radius:99px;background:var(--surface-3)}.progress-track i{height:100%;display:block;background:var(--accent);transition:width .12s linear}.error-banner,.notice-banner{padding:9px 12px;border-radius:9px;font-size:10px}.error-banner{background:color-mix(in srgb,var(--danger) 10%,transparent);color:var(--danger)}.notice-banner{background:color-mix(in srgb,var(--success) 10%,transparent);color:var(--success)}.modal-backdrop{position:fixed;inset:0;z-index:1200;display:grid;place-items:center;padding:20px;background:rgb(8 12 22 / 48%)}.modal{position:relative;width:min(590px,100%);padding:23px}.modal h2{margin:0 0 17px;font-size:27px;letter-spacing:-.04em}.modal>label{display:grid;gap:5px;margin-top:11px;color:var(--text-muted);font-size:10px}.modal input,.modal select,.picker{height:38px;padding:0 10px;border:1px solid var(--border);border-radius:9px;background:var(--bg-elevated);color:var(--text);font:inherit;text-align:left}.picker{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.toggle{display:flex!important;align-items:center;grid-template-columns:auto 1fr}.toggle input{width:17px;height:17px}.inline{display:grid;grid-template-columns:100px auto;align-items:center;gap:8px}.modal-actions{display:flex;gap:8px;margin-top:20px}.modal-actions button{flex:1}.close{position:absolute;right:12px;top:12px;width:30px;height:30px;border:0;border-radius:8px;background:var(--surface-2);color:var(--text)}.modal-lead{color:var(--text-muted);font-size:11px;line-height:1.6}.guide{display:flex;gap:10px;padding:10px 0}.guide>span{width:27px;height:27px;display:grid;place-items:center;flex:none;border-radius:8px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:900}.guide strong,.guide small{display:block}.guide strong{font-size:11px}.guide small{margin-top:3px;color:var(--text-muted);font-size:9px}.recipe-builder{width:min(720px,100%)}.builder-fields{display:grid;grid-template-columns:1fr 1fr;gap:8px}.builder-fields label{display:grid;gap:5px;color:var(--text-muted);font-size:10px}.add-step{display:grid;grid-template-columns:1fr auto;gap:7px;margin-top:14px}.add-step select{height:38px;padding:0 9px;border:1px solid var(--border);border-radius:9px;background:var(--bg-elevated);color:var(--text)}.builder-steps{max-height:300px;overflow:auto;margin-top:10px;border:1px solid var(--border);border-radius:10px}.builder-steps article{display:grid;grid-template-columns:30px minmax(0,1fr) auto;align-items:center;gap:8px;padding:8px;border-bottom:1px solid var(--border)}.builder-steps article:last-child{border-bottom:0}.builder-steps article>span{width:26px;height:26px;display:grid;place-items:center;border-radius:7px;background:var(--accent-soft);color:var(--accent);font-size:9px;font-weight:900}.builder-steps strong,.builder-steps small{display:block}.builder-steps strong{font-size:10px}.builder-steps small{margin-top:2px;color:var(--text-faint);font-size:8px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.step-buttons{display:flex;gap:3px}.step-buttons button{width:26px;height:26px;border:1px solid var(--border);border-radius:6px;background:var(--surface-2);color:var(--text-muted)}.builder-empty{padding:24px;color:var(--text-muted);font-size:10px;text-align:center}.builder-note{display:flex;justify-content:space-between;gap:10px;margin-top:10px;padding:8px 10px;border-radius:8px;background:var(--success-soft);color:var(--success);font-size:9px}.builder-note span{color:var(--text-muted)}@media(max-width:1050px){.template-grid{grid-template-columns:repeat(2,1fr)}.two-columns{grid-template-columns:1fr}}@media(max-width:700px){.page-head{align-items:flex-start;flex-direction:column}.status-grid{grid-template-columns:1fr 1fr}.template-grid{grid-template-columns:1fr}.recipes article{grid-template-columns:34px minmax(0,1fr) auto}.recipes .state{display:none}.progress-card{grid-template-columns:1fr}.section-title>span{display:none}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AutomationsPage {
  protected readonly store = inject(AutomationStore);
  protected readonly workspace = inject(WorkspaceStore);
  private readonly capabilities = inject(CapabilityStore);
  protected readonly builderOpen = signal(false);
  protected readonly watchOpen = signal(false);
  protected readonly watchPath = signal('');
  protected readonly watchRecipeId = signal('');
  protected readonly watchExtensions = signal('');
  protected readonly watchRecursive = signal(false);
  protected readonly watchStability = signal(3);
  protected readonly builderName = signal('');
  protected readonly builderDescription = signal('');
  protected readonly builderSteps = signal<string[]>([]);
  protected readonly executableBuilderActions = computed(() => this.capabilities.actions().filter((action) => this.capabilities.isActionExecutable(action) && action.batchable));
  protected readonly canRunWorkspace = computed(() => !!this.workspace.activeWorkspaceId());

  protected readonly templates: RecipeTemplate[] = [
    { id:'phone-photos', icon:'◉', name:'Photos prêtes à partager', description:'Uniformiser, alléger puis retirer les données privées.', trigger:'Images', steps:[
      {actionId:'image-batch-convert', label:'Convertir en JPEG', targetFormat:'jpg'},
      {actionId:'image-optimize', label:'Optimiser le poids', quality:'balanced'},
      {actionId:'strip-metadata', label:'Retirer GPS et métadonnées'},
    ]},
    { id:'scan-clean', icon:'Aa', name:'Scan propre et recherchable', description:'Transformer des scans en PDF exploitables.', trigger:'PDF', steps:[
      {actionId:'pdf-ocr', label:'OCR et PDF recherchable'},
      {actionId:'pdf-optimize-lossless', label:'Optimiser la structure'},
    ]},
    { id:'email-pdf', icon:'↓', name:'PDF pour e-mail', description:'Réduire le poids sans réglages techniques.', trigger:'PDF', steps:[
      {actionId:'pdf-compress', label:'Compression équilibrée', quality:'balanced'},
    ]},
    { id:'share-video', icon:'▶', name:'Vidéo universelle', description:'Préparer une vidéo compatible téléphone, web et messagerie.', trigger:'Vidéo', steps:[
      {actionId:'media-compatible', label:'MP4 H.264/AAC compatible', quality:'balanced'},
    ]},
  ];

  constructor() { this.store.load(); }

  protected async saveTemplate(template: RecipeTemplate): Promise<void> {
    const now = new Date().toISOString();
    const steps: WorkflowStep[] = template.steps.map((step, index) => ({
      id: `step-${index + 1}`,
      actionId: step.actionId,
      dependsOn: index === 0 ? [] : [`step-${index}`],
      targetFormat: step.targetFormat ?? null,
      quality: step.quality ?? null,
      parameters: step.parameters ?? {},
      outputPolicy: {
        destination: 'subfolder', customDirectory: null, subfolderName: 'FileFlow', preserveTree: true,
        conflict: 'increment', naming: 'operationSuffix', overwriteOriginal: false,
      },
    }));
    await this.store.save({
      id: crypto.randomUUID(), name: template.name, description: template.description, icon: template.icon,
      stepsJson: JSON.stringify({version: 1, name: template.name, description: template.description, steps}),
      enabled: true, createdAt: now, updatedAt: now,
    });
  }

  protected async runOnWorkspace(recipe: RecipeRecord): Promise<void> {
    const workspaceId = this.workspace.activeWorkspaceId();
    if (!workspaceId) return;
    await this.store.runWorkspace(recipe.id, workspaceId, [...this.workspace.selectedIds()]);
  }

  protected async chooseWatchFolder(): Promise<void> {
    const selected = await open({directory: true, multiple: false, title: 'Dossier à surveiller', canCreateDirectories: true});
    if (typeof selected === 'string') this.watchPath.set(selected);
  }

  protected async saveWatch(): Promise<void> {
    const recipeId = this.watchRecipeId();
    const path = this.watchPath();
    if (!recipeId || !path) return;
    const ok = await this.store.saveWatch({
      path, recipeId, enabled: true, recursive: this.watchRecursive(),
      extensions: this.watchExtensions().split(',').map((value) => value.trim()).filter(Boolean),
      stabilitySeconds: this.watchStability(),
    });
    if (ok) {
      this.watchOpen.set(false); this.watchPath.set(''); this.watchRecipeId.set(''); this.watchExtensions.set('');
      this.watchRecursive.set(false); this.watchStability.set(3);
    }
  }

  protected actionById(actionId: string): ActionDescriptor | null { return this.capabilities.action(actionId); }
  protected addBuilderStep(actionId: string): void { if (!this.capabilities.action(actionId)) return; this.builderSteps.update((steps) => [...steps, actionId].slice(0, 24)); }
  protected removeBuilderStep(index: number): void { this.builderSteps.update((steps) => steps.filter((_, current) => current !== index)); }
  protected moveBuilderStep(index: number, delta: number): void { this.builderSteps.update((steps) => { const target=index+delta;if(target<0||target>=steps.length)return steps;const next=[...steps];[next[index],next[target]]=[next[target],next[index]];return next; }); }
  protected closeBuilder(): void { this.builderOpen.set(false); this.builderName.set(''); this.builderDescription.set(''); this.builderSteps.set([]); }
  protected async saveCustomRecipe(): Promise<void> {
    const name=this.builderName().trim(); const actionIds=this.builderSteps(); if(!name||!actionIds.length)return;
    const now=new Date().toISOString();
    const steps:WorkflowStep[]=actionIds.map((actionId,index)=>({id:`step-${index+1}`,actionId,dependsOn:index===0?[]:[`step-${index}`],targetFormat:null,quality:null,parameters:{},outputPolicy:{destination:'subfolder',customDirectory:null,subfolderName:'FileFlow',preserveTree:true,conflict:'increment',naming:'operationSuffix',overwriteOriginal:false}}));
    const ok=await this.store.save({id:crypto.randomUUID(),name,description:this.builderDescription().trim()||'Recette FileFlow personnalisée',icon:'⚡',stepsJson:JSON.stringify({version:1,name,description:this.builderDescription().trim(),steps}),enabled:true,createdAt:now,updatedAt:now});
    if(ok)this.closeBuilder();
  }

  protected recipeName(recipeId?: string | null): string {
    return this.store.recipes().find((recipe) => recipe.id === recipeId)?.name ?? 'Workflow';
  }

  protected shortPath(path: string): string {
    const parts = path.split('/').filter(Boolean); return parts.length > 3 ? `…/${parts.slice(-3).join('/')}` : path;
  }
}
