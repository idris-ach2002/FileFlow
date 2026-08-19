import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import {
  ActionDescriptor,
  ActionRecommendation,
  ArchiveInspection,
  Asset,
  AssetSortKey,
  FormatFamily,
} from '../../core/ipc/tauri.models';
import { WorkspaceStore } from './data-access/workspace.store';

@Component({
  selector: 'ff-workspace-page',
  template: `
    <div class="workspace-shell">
      <header class="workspace-header">
        <div>
          <p class="ff-kicker">ESPACE DE TRAVAIL</p>
          <h1>{{ store.busy() ? 'Analyse en cours' : 'Vos fichiers sont prêts.' }}</h1>
          <p>{{ workspaceSubtitle() }}</p>
        </div>
        <div class="header-actions">
          <button class="ff-button secondary" type="button" (click)="newSelection()">＋ Ajouter</button>
        </div>
      </header>

      @if (store.error()) {
        <section class="error-state ff-card"><span>!</span><div><strong>Analyse interrompue</strong><p>{{ store.error() }}</p></div><button class="ff-button" type="button" (click)="newSelection()">Nouvelle sélection</button></section>
      } @else if (!store.hasWorkspace()) {
        <section class="empty-state ff-card"><div class="empty-icon">▦</div><h2>Aucun espace ouvert</h2><p>Ajoutez des fichiers ou un dossier pour commencer.</p><button class="ff-button" type="button" (click)="newSelection()">Choisir des fichiers</button></section>
      } @else {
        <section class="summary-grid">
          <article><span>Éléments</span><strong>{{ store.counts().assets }}</strong><small>{{ store.counts().files }} fichiers</small></article>
          <article><span>Taille totale</span><strong>{{ formatBytes(store.counts().totalBytes) }}</strong><small>originaux préservés</small></article>
          <article><span>Familles</span><strong>{{ store.workspace()?.families?.length ?? 0 }}</strong><small>types regroupés</small></article>
          <article><span>Archives</span><strong>{{ store.counts().archives }}</strong><small>inspectables</small></article>
          <article class="smart-stat"><span>Optimisation</span><strong>{{ formatBytes(store.insights()?.potentialDuplicateBytes ?? 0) }}</strong><small>doublons potentiels</small></article>
        </section>

        @if (store.busy()) {
          <section class="scan-status ff-card">
            <div class="scan-spinner"></div>
            <div class="scan-copy"><strong>Analyse asynchrone</strong><span>{{ store.stats().discovered }} éléments · {{ formatBytes(store.stats().totalBytes) }}</span></div>
            <div class="scan-track"><span></span></div>
            <small>L’interface reste disponible pendant le scan.</small>
          </section>
        }

        <div class="workspace-layout" [class.action-open]="activeAction()">
          <section class="files-column">
            <div class="workspace-toolbar ff-card">
              <label class="search-field"><span>⌕</span><input #search type="search" placeholder="Rechercher dans ce workspace…" [value]="store.searchTerm()" (input)="searchChanged(search.value)" /></label>
              <div class="toolbar-separator"></div>
              <button class="toolbar-button" type="button" [class.active]="store.sortBy() === 'name'" (click)="sort('name')">Nom {{ sortArrow('name') }}</button>
              <button class="toolbar-button" type="button" [class.active]="store.sortBy() === 'size'" (click)="sort('size')">Taille {{ sortArrow('size') }}</button>
              <button class="toolbar-button icon-only" type="button" [class.active]="!store.includeHidden()" (click)="toggleHidden()" title="Afficher ou masquer les éléments cachés">◌</button>
            </div>

            @if (store.workspace(); as workspace) {
              <nav class="family-filters" aria-label="Filtrer les fichiers par type">
                <button type="button" [class.active]="store.familyFilter() === null" (click)="filterFamily(null)">Tous <span>{{ workspace.counts.files + workspace.counts.archives }}</span></button>
                @for (entry of workspace.families; track entry.family) {
                  <button type="button" [class.active]="store.familyFilter() === entry.family" (click)="filterFamily(entry.family)">{{ familyLabel(entry.family) }} <span>{{ entry.count }}</span></button>
                }
              </nav>
            }

            @if (store.selectedCount() > 0) {
              <div class="selection-bar">
                <div><strong>{{ store.selectedCount() }} sélectionné{{ store.selectedCount() > 1 ? 's' : '' }}</strong><span>Les actions compatibles seront appliquées uniquement à cette sélection.</span></div>
                <button class="ff-button ghost" type="button" (click)="store.clearSelection()">Effacer</button>
                <button class="ff-button secondary" type="button" (click)="store.selectVisible()">Tout sélectionner</button>
              </div>
            }

            <section class="asset-panel ff-card">
              <div class="asset-panel-head">
                <div><strong>{{ store.pageTotal() || store.assets().length }} éléments</strong><span>{{ store.searchTerm() ? 'Résultats filtrés' : 'Workspace local' }}</span></div>
                <div class="panel-badges">@if (store.warnings().length) { <span class="ff-badge warning">{{ store.warnings().length }} avert.</span> }<span class="ff-badge success">● Local</span></div>
              </div>

              <div class="asset-list">
                @for (asset of store.assets(); track asset.data.id) {
                  <article class="asset-row" [class.selected]="store.isSelected(asset.data.id)" (dblclick)="toggleAsset(asset)">
                    <label class="asset-check" title="Sélectionner"><input type="checkbox" [checked]="store.isSelected(asset.data.id)" (change)="toggleAsset(asset)" /><span></span></label>
                    <div class="asset-icon" [attr.data-family]="assetFamily(asset)">{{ assetMark(asset) }}</div>
                    <div class="asset-main"><strong>{{ asset.data.name }}</strong><span>{{ asset.data.relativePath }}</span></div>
                    <div class="asset-format"><strong>{{ assetFormat(asset) }}</strong><span>{{ familyLabel(assetFamily(asset)) }}</span></div>
                    <div class="asset-size">{{ assetSize(asset) }}</div>
                    <button class="row-more" type="button" title="Actions pour ce fichier" (click)="selectAndSuggest(asset)">•••</button>
                  </article>
                } @empty {
                  <div class="asset-empty"><span>⌕</span><strong>Aucun élément</strong><small>Modifiez la recherche ou le filtre.</small></div>
                }
              </div>

              @if (store.hasMore()) { <div class="load-more"><button class="ff-button secondary" type="button" (click)="loadMore()">Afficher 200 éléments supplémentaires</button></div> }
            </section>
          </section>

          <aside class="intelligence-column">
            <section class="recommendation-panel ff-card">
              <div class="panel-title"><div><p class="ff-kicker">SUGGESTIONS</p><h2>Actions pertinentes</h2></div><span class="spark">✦</span></div>
              <div class="recommendation-list">
                @for (recommendation of store.recommendations().slice(0, 6); track recommendation.actionId) {
                  @if (actionFor(recommendation); as action) {
                    <button type="button" class="recommendation" [class.not-ready]="!recommendation.ready" (click)="openAction(action)">
                      <span class="rec-mark" [attr.data-category]="action.category">{{ actionMark(action) }}</span>
                      <span class="rec-copy"><strong>{{ action.title }}</strong><small>{{ recommendation.reason }}</small><em>{{ recommendation.affectedAssets }} élément{{ recommendation.affectedAssets > 1 ? 's' : '' }}</em></span>
                      <span class="rec-arrow">›</span>
                    </button>
                  }
                } @empty {
                  <div class="recommendation-empty">Les recommandations apparaîtront à la fin de l’analyse.</div>
                }
              </div>
            </section>

            @if (store.insights(); as insights) {
              <section class="insight-panel ff-card">
                <div class="panel-title compact"><div><p class="ff-kicker">APERÇU</p><h2>Ce que FileFlow voit</h2></div></div>
                @if (insights.largest.length) {
                  <div class="insight-block"><span>Plus gros fichiers</span>@for (item of insights.largest.slice(0, 3); track item.id) { <div class="insight-row"><div><strong>{{ item.name }}</strong><small>{{ familyLabel(item.family) }}</small></div><b>{{ formatBytes(item.sizeBytes) }}</b></div> }</div>
                }
                @if (insights.duplicateSizeCandidates.length) {
                  <div class="duplicate-hint"><span class="duplicate-icon">≋</span><div><strong>{{ insights.duplicateSizeCandidates.length }} groupes à vérifier</strong><small>Jusqu’à {{ formatBytes(insights.potentialDuplicateBytes) }} potentiellement récupérables. Une empreinte cryptographique SHA-256 confirmera les vrais doublons.</small>@if (store.duplicateReport(); as duplicateReport) { <em>{{ duplicateReport.confirmedGroups.length }} groupe(s) confirmé(s) · {{ formatBytes(duplicateReport.reclaimableBytes) }} récupérables</em> } @else { <button type="button" [disabled]="store.duplicateScanLoading()" (click)="store.confirmDuplicates()">{{ store.duplicateScanLoading() ? 'Analyse…' : 'Confirmer les doublons' }}</button> }</div></div>
                  @if (store.duplicateScanError()) { <div class="mini-error">{{ store.duplicateScanError() }}</div> }
                }
                @if (store.counts().archives > 0) {
                  <div class="archive-inspector">
                    <div class="archive-inspector-head"><span>ZIP</span><div><strong>Contenu des archives</strong><small>Inspecter sans extraire pour connaître les types présents.</small></div></div>
                    @if (store.archiveInspection(); as archive) {
                      <div class="archive-metrics"><b>{{ archive.files }} fichiers</b><span>{{ archive.directories }} dossiers</span><span>{{ formatBytes(archive.totalUnpackedBytes) }}</span></div>
                      <div class="archive-families">@for (family of archive.families.slice(0, 6); track family.family) { <span>{{ familyLabel(family.family) }} <b>{{ family.count }}</b></span> }</div>
                      @if (archive.samples.length) { <small class="archive-sample">Ex. {{ archiveSamplePaths(archive) }}</small> }
                    } @else {
                      <button type="button" [disabled]="store.archiveInspectionLoading()" (click)="store.inspectArchive()">{{ store.archiveInspectionLoading() ? 'Inspection…' : 'Inspecter la première archive' }}</button>
                    }
                  </div>
                  @if (store.archiveInspectionError()) { <div class="mini-error">{{ store.archiveInspectionError() }}</div> }
                }
                <div class="extension-cloud">@for (extension of insights.extensions.slice(0, 8); track extension.extension) { <span>{{ extension.extension.toUpperCase() }} <b>{{ extension.count }}</b></span> }</div>
              </section>
            }
          </aside>

          @if (activeAction(); as action) {
            <aside class="action-drawer ff-card">
              <button class="drawer-close" type="button" (click)="store.closeAction()" aria-label="Fermer">×</button>
              <div class="drawer-mark" [attr.data-category]="action.category">{{ actionMark(action) }}</div>
              <p class="ff-kicker">ACTION</p><h2>{{ action.title }}</h2><p class="drawer-description">{{ action.description }}</p>
              <div class="drawer-status" [class.warning]="capabilities.actionState(action) !== 'ready'"><span>{{ capabilities.actionState(action) === 'ready' ? '●' : '!' }}</span><div><strong>{{ actionRuntimeLabel(action) }}</strong><small>{{ engineStatus(action) }}</small></div></div>
              <div class="drawer-section"><label>Portée</label><div class="scope-card"><strong>{{ store.selectedCount() ? store.selectedCount() + ' sélectionné(s)' : 'Tout le groupe compatible' }}</strong><span>{{ action.batchable ? 'Traitement par lot · scheduler adaptatif' : 'Une opération à la fois' }}</span></div></div>
              @if (targetOptions(action).length) {
                <div class="drawer-section"><label>Format de sortie</label><div class="format-grid">@for (format of targetOptions(action); track format) { <button type="button" [class.active]="effectiveTarget(action) === format" (click)="targetFormat.set(format)">{{ format.toUpperCase() }}</button> }</div></div>
              }
              @if (supportsQuality(action)) {
                <div class="drawer-section"><label>Profil</label><div class="segmented"><button type="button" [class.active]="quality() === 'small'" (click)="quality.set('small')">Petit</button><button type="button" [class.active]="quality() === 'balanced'" (click)="quality.set('balanced')">Équilibré</button><button type="button" [class.active]="quality() === 'high'" (click)="quality.set('high')">Qualité</button></div></div>
              }
              <div class="drawer-section"><label>Destination du résultat</label><div class="segmented"><button type="button" [class.active]="destination() === 'subfolder'" (click)="destination.set('subfolder')">Sous-dossier</button><button type="button" [class.active]="destination() === 'same'" (click)="destination.set('same')">Même dossier</button><button type="button" [class.active]="destination() === 'choose'" (click)="chooseDestination()">Choisir</button></div>@if (customDirectory()) { <div class="destination-path">{{ customDirectory() }}</div> }</div>
              <div class="drawer-section"><label>Organisation</label><label class="toggle-line"><input type="checkbox" [checked]="preserveTree()" (change)="preserveTree.set(!preserveTree())" /><span>Conserver l’arborescence des dossiers</span></label></div>
              <div class="drawer-section"><label>Protection</label><div class="safety-list"><span><b>✓</b> Originaux conservés</span><span><b>✓</b> Conflits renommés automatiquement</span><span><b>✓</b> Temporaire + finalisation atomique</span><span><b>✓</b> Processus isolé et annulable</span></div></div>
              @if (store.executing()) {
                <div class="job-box"><div><strong>Traitement en cours</strong><span>{{ store.executionCompleted() }} / {{ store.executionTotal() }}</span></div><div class="job-progress"><i [style.width.%]="store.executionProgress()"></i></div><small>Le scheduler protège CPU, mémoire et E/S pendant l’opération.</small></div>
              }
              @if (store.executionSummary(); as summary) {
                <div class="result-box" [class.failed]="summary.state === 'failed'">
                  <strong>{{ summary.state === 'completed' ? 'Terminé' : summary.state === 'cancelled' ? 'Annulé' : 'Terminé avec erreurs' }}</strong>
                  <span>{{ summary.succeeded }} réussi(s) · {{ summary.skipped }} ignoré(s) · {{ summary.failed }} échec(s)</span>
                  @if (summary.outputs.length) {
                    <small>{{ summary.outputs[0] }}</small>
                    @if (summary.outputs.length > 1) { <em>+ {{ summary.outputs.length - 1 }} autre{{ summary.outputs.length > 2 ? 's' : '' }} résultat{{ summary.outputs.length > 2 ? 's' : '' }}</em> }
                    <div class="result-actions">
                      <button type="button" [disabled]="store.outputActionBusy()" (click)="store.openOutput()">Ouvrir</button>
                      <button type="button" [disabled]="store.outputActionBusy()" (click)="store.revealOutput()">Afficher</button>
                      <button type="button" [disabled]="store.outputActionBusy()" (click)="store.saveOutputCopy()">Enregistrer une copie…</button>
                      @if (action.id === 'archive-extract') { <button type="button" [disabled]="store.outputActionBusy()" (click)="store.analyzeOutput()">Analyser le dossier extrait</button> }
                    </div>
                  }
                </div>
              }
              @if (store.outputActionMessage()) { <div class="action-notice">{{ store.outputActionMessage() }}</div> }
              @if (store.executionError()) { <div class="action-notice error">{{ store.executionError() }}</div> }
              @if (capabilities.actionState(action) === 'planned') { <div class="action-notice">Cette action fait déjà partie du moteur de capacités, mais son exécuteur local n’est pas encore activé dans cette build.</div> }
              <div class="drawer-footer">@if (store.executing()) { <button class="ff-button danger" type="button" (click)="store.cancelExecution()">Annuler le traitement</button> } @else { <button class="ff-button secondary" type="button" (click)="store.closeAction()">Fermer</button><button class="ff-button" type="button" [disabled]="!capabilities.isActionExecutable(action)" (click)="runAction(action)">Lancer</button> }</div>
            </aside>
          }
        </div>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.workspace-shell{max-width:1460px;margin:0 auto}.workspace-header{display:flex;justify-content:space-between;gap:24px;align-items:flex-end}.workspace-header h1{margin:0;color:var(--text-strong);font-size:clamp(34px,4.5vw,52px);letter-spacing:-.05em}.workspace-header>div>p:last-child{max-width:760px;margin:10px 0 0;color:var(--text-muted);font-size:13px;line-height:1.6}.header-actions{display:flex;gap:8px}.summary-grid{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:9px;margin-top:28px}.summary-grid article{min-height:92px;display:grid;align-content:center;gap:4px;padding:15px 16px;border:1px solid var(--border);border-radius:14px;background:var(--surface-1);box-shadow:var(--shadow-sm)}.summary-grid span,.summary-grid small{color:var(--text-muted);font-size:10px}.summary-grid strong{color:var(--text-strong);font-size:21px;letter-spacing:-.035em}.smart-stat{background:linear-gradient(145deg,var(--surface-1),var(--success-soft))!important}.scan-status{margin-top:12px;display:grid;grid-template-columns:auto minmax(0,1fr) 180px auto;align-items:center;gap:12px;padding:11px 14px}.scan-spinner{width:20px;height:20px;border:2px solid var(--border-strong);border-top-color:var(--accent);border-radius:50%;animation:spin .8s linear infinite}@keyframes spin{to{transform:rotate(360deg)}}.scan-copy strong,.scan-copy span{display:block}.scan-copy strong{font-size:11px}.scan-copy span,.scan-status small{margin-top:2px;color:var(--text-muted);font-size:11px}.scan-track{height:4px;overflow:hidden;border-radius:99px;background:var(--surface-3)}.scan-track span{display:block;width:42%;height:100%;border-radius:inherit;background:var(--accent);animation:scan 1.1s ease-in-out infinite alternate}@keyframes scan{to{transform:translateX(138%)}}
    .workspace-layout{margin-top:20px;display:grid;grid-template-columns:minmax(0,1fr) 330px;gap:14px;align-items:start}.workspace-layout.action-open{grid-template-columns:minmax(0,1fr) 300px 340px}.files-column{min-width:0}.workspace-toolbar{min-height:50px;display:flex;align-items:center;gap:6px;padding:6px}.search-field{min-width:190px;flex:1;display:flex;align-items:center;gap:7px;padding:0 8px;color:var(--text-faint)}.search-field input{width:100%;min-height:36px;border:0;outline:0;background:transparent;color:var(--text);font-size:11px}.toolbar-separator{width:1px;height:24px;background:var(--border)}.toolbar-button{min-height:35px;padding:0 10px;border:0;border-radius:8px;background:transparent;color:var(--text-muted);font-size:10px;font-weight:750}.toolbar-button:hover,.toolbar-button.active{background:var(--surface-2);color:var(--text)}.toolbar-button.icon-only{width:35px;padding:0}.family-filters{display:flex;gap:6px;overflow:auto;padding:10px 1px 8px;scrollbar-width:none}.family-filters button{flex:none;min-height:31px;padding:0 10px;border:1px solid var(--border);border-radius:999px;background:var(--surface-1);color:var(--text-muted);font-size:10px;font-weight:750}.family-filters button span{margin-left:5px;color:var(--text-faint)}.family-filters button.active{border-color:color-mix(in srgb,var(--accent) 35%,var(--border));background:var(--accent-soft);color:var(--accent-strong)}.selection-bar{min-height:52px;display:flex;align-items:center;gap:8px;margin-bottom:8px;padding:7px 9px 7px 13px;border:1px solid color-mix(in srgb,var(--accent) 28%,var(--border));border-radius:12px;background:var(--accent-soft)}.selection-bar>div{flex:1}.selection-bar strong,.selection-bar span{display:block}.selection-bar strong{font-size:11px}.selection-bar span{margin-top:2px;color:var(--text-muted);font-size:11px}.selection-bar .ff-button{min-height:32px;font-size:10px}
    .asset-panel{overflow:hidden}.asset-panel-head{min-height:54px;display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 13px;border-bottom:1px solid var(--border)}.asset-panel-head strong,.asset-panel-head span{display:block}.asset-panel-head strong{font-size:11px}.asset-panel-head div>span{margin-top:2px;color:var(--text-muted);font-size:11px}.panel-badges{display:flex!important;gap:5px}.asset-list{display:grid}.asset-row{min-height:62px;display:grid;grid-template-columns:25px 38px minmax(120px,1fr) 100px 76px 30px;align-items:center;gap:9px;padding:7px 10px;border-bottom:1px solid var(--border);transition:background var(--transition)}.asset-row:hover{background:var(--bg-elevated)}.asset-row.selected{background:var(--accent-soft)}.asset-check{width:22px;height:22px;display:grid;place-items:center;cursor:pointer}.asset-check input{position:absolute;opacity:0;pointer-events:none}.asset-check span{width:14px;height:14px;border:1.5px solid var(--border-strong);border-radius:4px;background:var(--surface-1)}.asset-check input:checked+span{border-color:var(--accent);background:var(--accent);box-shadow:inset 0 0 0 3px var(--surface-1)}.asset-icon{width:36px;height:36px;display:grid;place-items:center;border-radius:10px;background:var(--surface-2);color:var(--text-muted);font-size:11px;font-weight:900;text-transform:uppercase}.asset-icon[data-family='image']{background:#eaf6ff;color:#2875bb}.asset-icon[data-family='pdf']{background:#fff0ef;color:#c14f46}.asset-icon[data-family='archive']{background:var(--warning-soft);color:var(--warning)}.asset-icon[data-family='video'],.asset-icon[data-family='audio']{background:#f2ecff;color:#7450b6}.asset-main,.asset-format{min-width:0}.asset-main strong,.asset-main span,.asset-format strong,.asset-format span{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.asset-main strong{font-size:11px}.asset-main span,.asset-format span{margin-top:3px;color:var(--text-faint);font-size:11px}.asset-format strong{font-size:11px;text-transform:uppercase}.asset-size{color:var(--text-muted);font-size:10px;text-align:right}.row-more{width:30px;height:30px;border:0;border-radius:8px;background:transparent;color:var(--text-faint);font-size:11px}.row-more:hover{background:var(--surface-2);color:var(--text)}.asset-empty{min-height:210px;display:grid;place-items:center;align-content:center;gap:5px;color:var(--text-faint)}.asset-empty>span{font-size:24px}.asset-empty strong{color:var(--text-muted);font-size:12px}.asset-empty small{font-size:10px}.load-more{display:flex;justify-content:center;padding:10px;border-top:1px solid var(--border)}.load-more .ff-button{min-height:34px;font-size:10px}
    .intelligence-column{display:grid;gap:10px;position:sticky;top:24px}.recommendation-panel,.insight-panel{overflow:hidden;padding:14px}.panel-title{display:flex;justify-content:space-between;align-items:flex-start;padding:2px 2px 11px}.panel-title h2{margin:0;font-size:16px;letter-spacing:-.03em}.panel-title.compact{padding-bottom:5px}.spark{width:29px;height:29px;display:grid;place-items:center;border-radius:9px;background:var(--accent-soft);color:var(--accent)}.recommendation-list{display:grid;gap:4px}.recommendation{width:100%;min-height:66px;display:grid;grid-template-columns:35px minmax(0,1fr) 14px;align-items:center;gap:9px;padding:7px;border:0;border-radius:10px;background:transparent;color:var(--text);text-align:left}.recommendation:hover{background:var(--surface-2)}.recommendation.not-ready{opacity:.63}.rec-mark{width:34px;height:34px;display:grid;place-items:center;border-radius:9px;background:var(--accent-soft);color:var(--accent);font-size:11px;font-weight:900}.rec-copy{min-width:0}.rec-copy strong,.rec-copy small,.rec-copy em{display:block}.rec-copy strong{font-size:10px}.rec-copy small{margin-top:3px;display:-webkit-box;overflow:hidden;color:var(--text-muted);font-size:11px;line-height:1.35;-webkit-box-orient:vertical;-webkit-line-clamp:2}.rec-copy em{margin-top:4px;color:var(--text-faint);font-size:10px;font-style:normal}.rec-arrow{color:var(--text-faint);font-size:18px}.recommendation-empty{padding:24px 10px;color:var(--text-muted);font-size:10px;line-height:1.5;text-align:center}.insight-block{margin-top:4px;padding-top:10px;border-top:1px solid var(--border)}.insight-block>span{color:var(--text-faint);font-size:11px;font-weight:800;text-transform:uppercase}.insight-row{display:flex;justify-content:space-between;gap:8px;padding:8px 0;border-bottom:1px solid var(--border)}.insight-row div{min-width:0}.insight-row strong,.insight-row small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.insight-row strong{font-size:11px}.insight-row small{margin-top:2px;color:var(--text-faint);font-size:10px}.insight-row b{flex:none;color:var(--text-muted);font-size:11px}.duplicate-hint{display:flex;gap:9px;margin-top:10px;padding:10px;border-radius:10px;background:var(--warning-soft)}.duplicate-icon{width:28px;height:28px;display:grid;place-items:center;flex:none;border-radius:8px;background:color-mix(in srgb,var(--warning) 12%,transparent);color:var(--warning)}.duplicate-hint strong,.duplicate-hint small,.duplicate-hint em{display:block}.duplicate-hint strong{font-size:11px}.duplicate-hint small{margin-top:3px;color:var(--text-muted);font-size:10px;line-height:1.4}.duplicate-hint button{margin-top:7px;padding:5px 7px;border:0;border-radius:6px;background:color-mix(in srgb,var(--warning) 15%,transparent);color:var(--warning);font-size:10px;font-weight:800}.duplicate-hint button:disabled{opacity:.6}.duplicate-hint em{margin-top:6px;color:var(--success);font-size:10px;font-style:normal;font-weight:800}.mini-error{margin-top:6px;color:var(--danger);font-size:10px}.archive-inspector{margin-top:10px;padding:10px;border:1px solid var(--border);border-radius:10px;background:var(--bg-elevated)}.archive-inspector-head{display:flex;gap:9px;align-items:flex-start}.archive-inspector-head>span{width:31px;height:31px;display:grid;place-items:center;flex:none;border-radius:8px;background:var(--warning-soft);color:var(--warning);font-size:10px;font-weight:900}.archive-inspector-head strong,.archive-inspector-head small{display:block}.archive-inspector-head strong{font-size:11px}.archive-inspector-head small{margin-top:3px;color:var(--text-muted);font-size:10px;line-height:1.35}.archive-inspector>button{margin-top:9px;padding:6px 9px;border:0;border-radius:7px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:800}.archive-metrics{display:flex;flex-wrap:wrap;gap:8px;margin-top:9px;color:var(--text-muted);font-size:10px}.archive-metrics b{color:var(--text)}.archive-families{display:flex;flex-wrap:wrap;gap:4px;margin-top:8px}.archive-families span{padding:4px 6px;border-radius:6px;background:var(--surface-2);color:var(--text-muted);font-size:10px}.archive-families b{color:var(--text)}.archive-sample{display:block;margin-top:8px;color:var(--text-faint);font-size:10px;line-height:1.45;overflow-wrap:anywhere}.extension-cloud{display:flex;flex-wrap:wrap;gap:4px;margin-top:10px}.extension-cloud span{padding:4px 6px;border-radius:6px;background:var(--surface-2);color:var(--text-muted);font-size:10px;font-weight:750}.extension-cloud b{color:var(--text-faint)}
    .action-drawer{position:sticky;top:24px;padding:18px}.drawer-close{position:absolute;top:10px;right:10px;width:30px;height:30px;border:0;border-radius:8px;background:transparent;color:var(--text-muted);font-size:20px}.drawer-close:hover{background:var(--surface-2)}.drawer-mark{width:46px;height:46px;display:grid;place-items:center;margin-bottom:18px;border-radius:14px;background:var(--accent-soft);color:var(--accent);font-size:11px;font-weight:900}.action-drawer h2{margin:0;font-size:24px;letter-spacing:-.04em}.drawer-description{margin:8px 0 0;color:var(--text-muted);font-size:11px;line-height:1.55}.drawer-status{display:flex;gap:9px;margin-top:16px;padding:10px;border-radius:10px;background:var(--success-soft);color:var(--success)}.drawer-status.warning{background:var(--warning-soft);color:var(--warning)}.drawer-status>span{font-size:11px}.drawer-status strong,.drawer-status small{display:block}.drawer-status strong{font-size:11px}.drawer-status small{margin-top:2px;color:var(--text-muted);font-size:10px}.drawer-section{margin-top:18px}.drawer-section>label{display:block;margin-bottom:7px;color:var(--text-faint);font-size:10px;font-weight:850;text-transform:uppercase;letter-spacing:.08em}.scope-card{padding:10px;border:1px solid var(--border);border-radius:9px;background:var(--bg-elevated)}.scope-card strong,.scope-card span{display:block}.scope-card strong{font-size:10px}.scope-card span{margin-top:3px;color:var(--text-muted);font-size:10px}.segmented{display:grid;grid-template-columns:repeat(3,1fr);padding:3px;border-radius:9px;background:var(--surface-2)}.segmented button{min-height:30px;border:0;border-radius:7px;background:transparent;color:var(--text-muted);font-size:10px;font-weight:750}.segmented button.active{background:var(--surface-1);color:var(--text);box-shadow:var(--shadow-sm)}.safety-list{display:grid;gap:6px;color:var(--text-muted);font-size:11px}.safety-list b{margin-right:5px;color:var(--success)}.format-grid{display:grid;grid-template-columns:repeat(4,1fr);gap:5px}.format-grid button{min-height:31px;border:1px solid var(--border);border-radius:8px;background:var(--bg-elevated);color:var(--text-muted);font-size:10px;font-weight:850}.format-grid button.active{border-color:var(--accent);background:var(--accent-soft);color:var(--accent)}.destination-path{margin-top:7px;padding:7px 8px;border-radius:7px;background:var(--surface-2);color:var(--text-muted);font-size:10px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.toggle-line{display:flex;align-items:center;gap:8px;color:var(--text-muted);font-size:11px}.toggle-line input{accent-color:var(--accent)}.job-box,.result-box{margin-top:12px;padding:10px;border:1px solid var(--border);border-radius:10px;background:var(--surface-2)}.job-box>div:first-child{display:flex;justify-content:space-between;gap:8px}.job-box strong,.job-box span,.job-box small,.result-box strong,.result-box span,.result-box small,.result-box em{display:block}.job-box strong,.result-box strong{font-size:11px}.job-box span,.result-box span{color:var(--text-muted);font-size:10px}.job-box small,.result-box small{margin-top:7px;color:var(--text-faint);font-size:10px;line-height:1.4;overflow-wrap:anywhere}.result-box em{margin-top:4px;color:var(--text-muted);font-size:10px;font-style:normal}.result-actions{display:flex;flex-wrap:wrap;gap:5px;margin-top:9px}.result-actions button{min-height:28px;padding:0 8px;border:1px solid color-mix(in srgb,var(--success) 22%,transparent);border-radius:7px;background:var(--bg-elevated);color:var(--text);font-size:10px;font-weight:800}.result-actions button:hover{border-color:var(--success)}.result-actions button:disabled{opacity:.5}.job-progress{height:5px;margin-top:8px;overflow:hidden;border-radius:99px;background:var(--border)}.job-progress i{display:block;height:100%;border-radius:inherit;background:var(--accent);transition:width .2s ease}.result-box{background:var(--success-soft);border-color:transparent;color:var(--success)}.result-box.failed{background:var(--danger-soft);color:var(--danger)}.action-notice.error{background:var(--danger-soft);color:var(--danger)}.ff-button.danger{background:var(--danger);color:white}.drawer-footer{display:flex;gap:7px;margin-top:22px}.drawer-footer .ff-button{flex:1;min-height:36px;padding-inline:8px;font-size:11px}.action-notice{margin-top:8px;padding:8px;border-radius:8px;background:var(--accent-soft);color:var(--accent-strong);font-size:10px;line-height:1.4}.error-state{display:flex;align-items:center;gap:13px;margin-top:50px;padding:18px}.error-state>span{width:36px;height:36px;display:grid;place-items:center;border-radius:10px;background:var(--danger-soft);color:var(--danger);font-weight:900}.error-state div{flex:1}.error-state strong,.error-state p{display:block}.error-state p{margin:3px 0 0;color:var(--text-muted);font-size:10px}.empty-state{max-width:620px;margin:80px auto 0;display:grid;justify-items:center;gap:8px;padding:46px;text-align:center}.empty-state h2{margin:4px 0 0}.empty-state p{margin:0 0 8px;color:var(--text-muted);font-size:11px}.empty-icon{width:50px;height:50px;display:grid;place-items:center;border-radius:15px;background:var(--accent-soft);color:var(--accent);font-size:22px}
    @media(max-width:1320px){.workspace-layout,.workspace-layout.action-open{grid-template-columns:minmax(0,1fr) 300px}.action-drawer{grid-column:1/-1;position:relative;top:0;display:grid;grid-template-columns:80px minmax(0,1fr) 300px;gap:8px 16px}.action-drawer>.drawer-mark{grid-row:1/5}.action-drawer>.ff-kicker,.action-drawer>h2,.action-drawer>.drawer-description{grid-column:2}.drawer-status{grid-column:3;grid-row:1/3;margin-top:0}.drawer-section,.drawer-footer,.action-notice{grid-column:2/-1}.intelligence-column{position:static}}@media(max-width:1000px){.summary-grid{grid-template-columns:repeat(3,1fr)}.workspace-layout,.workspace-layout.action-open{grid-template-columns:1fr}.intelligence-column{grid-template-columns:repeat(2,1fr)}.asset-row{grid-template-columns:25px 38px minmax(100px,1fr) 70px 28px}.asset-format{display:none}.action-drawer{display:block}.action-drawer>.drawer-mark{margin-bottom:16px}.drawer-status{margin-top:16px}}@media(max-width:680px){.workspace-header{align-items:flex-start}.workspace-header h1{font-size:34px}.summary-grid{grid-template-columns:repeat(2,1fr)}.summary-grid article:last-child{display:none}.scan-status{grid-template-columns:auto 1fr}.scan-track,.scan-status>small{display:none}.intelligence-column{grid-template-columns:1fr}.workspace-toolbar{flex-wrap:wrap}.search-field{flex-basis:100%}.toolbar-separator{display:none}.asset-row{grid-template-columns:22px 36px minmax(0,1fr) 28px}.asset-size{display:none}.selection-bar{flex-wrap:wrap}.selection-bar>div{flex-basis:100%}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WorkspacePage {
  protected readonly store = inject(WorkspaceStore);
  protected readonly capabilities = inject(CapabilityStore);
  private readonly router = inject(Router);
  protected readonly destination = signal<'subfolder' | 'same' | 'choose'>('subfolder');
  protected readonly customDirectory = signal<string | null>(null);
  protected readonly targetFormat = signal<string | null>(null);
  protected readonly quality = signal<'small' | 'balanced' | 'high'>('balanced');
  protected readonly preserveTree = signal(true);
  private searchTimer?: ReturnType<typeof setTimeout>;

  protected readonly activeAction = computed(() => this.capabilities.action(this.store.activeActionId()));

  protected newSelection(): void { void this.router.navigate(['/']); }
  protected filterFamily(family: FormatFamily | null): void { void this.store.setFamilyFilter(family); }
  protected loadMore(): void { void this.store.loadMore(); }
  protected sort(sort: AssetSortKey): void { void this.store.setSort(sort); }
  protected toggleHidden(): void { void this.store.setIncludeHidden(!this.store.includeHidden()); }
  protected toggleAsset(asset: Asset): void { this.store.toggleSelection(asset.data.id); }

  protected searchChanged(value: string): void {
    if (this.searchTimer) clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.store.setSearch(value), 180);
  }

  protected selectAndSuggest(asset: Asset): void {
    if (!this.store.isSelected(asset.data.id)) this.store.toggleSelection(asset.data.id);
    const family = this.assetFamily(asset);
    const candidate = this.capabilities.actions().find((action) => action.accepts.includes(family) && action.featured);
    if (candidate) this.openAction(candidate);
  }

  protected openAction(action: ActionDescriptor): void {
    this.targetFormat.set(defaultTarget(action));
    this.quality.set('balanced');
    this.store.openAction(action.id);
  }
  protected actionFor(recommendation: ActionRecommendation): ActionDescriptor | null { return this.capabilities.action(recommendation.actionId); }

  protected async chooseDestination(): Promise<void> {
    const paths = await this.store.pickDirectories();
    if (!paths.length) return;
    this.customDirectory.set(paths[0]);
    this.destination.set('choose');
  }

  protected async runAction(action: ActionDescriptor): Promise<void> {
    const workspaceId = this.store.workspace()?.id;
    if (!workspaceId || !this.capabilities.isActionExecutable(action)) return;
    if (this.destination() === 'choose' && !this.customDirectory()) {
      await this.chooseDestination();
      if (!this.customDirectory()) return;
    }
    await this.store.executeAction({
      workspaceId,
      actionId: action.id,
      selectedAssetIds: [...this.store.selectedIds()],
      targetFormat: this.effectiveTarget(action),
      quality: this.supportsQuality(action) ? this.quality() : null,
      outputPolicy: {
        destination: this.destination() === 'subfolder' ? 'subfolder' : this.destination() === 'same' ? 'sameFolder' : 'customFolder',
        customDirectory: this.destination() === 'choose' ? this.customDirectory() : null,
        subfolderName: 'FileFlow',
        preserveTree: this.preserveTree(),
        conflict: 'increment',
        naming: 'original',
        overwriteOriginal: false,
      },
    });
  }

  protected targetOptions(action: ActionDescriptor): string[] { return TARGET_OPTIONS[action.id] ?? []; }
  protected effectiveTarget(action: ActionDescriptor): string | null { return this.targetFormat() ?? defaultTarget(action); }
  protected supportsQuality(action: ActionDescriptor): boolean { return QUALITY_ACTIONS.has(action.id); }
  protected actionRuntimeLabel(action: ActionDescriptor): string {
    const state = this.capabilities.actionState(action);
    return state === 'ready' ? 'Exécution locale prête' : state === 'missing-engine' ? 'Dépendance manquante' : 'Pipeline planifié';
  }

  protected engineStatus(action: ActionDescriptor): string {
    const missing = this.capabilities.missingEngines(action);
    if (!missing.length) return action.requiredEngines.length ? action.requiredEngines.join(' · ') : 'Fonction native FileFlow';
    return `Manquant : ${missing.join(', ')}`;
  }

  protected sortArrow(key: AssetSortKey): string { return this.store.sortBy() === key ? (this.store.sortDirection() === 'ascending' ? '↑' : '↓') : ''; }
  protected workspaceSubtitle(): string { const roots = this.store.workspace()?.roots.length ?? 0; return this.store.busy() ? 'Les résultats apparaissent progressivement, sans charger tout le dossier en mémoire.' : `${roots} source${roots > 1 ? 's' : ''} analysée${roots > 1 ? 's' : ''} · recommandations calculées localement.`; }
  protected familyLabel(family: FormatFamily): string { return FAMILY_LABELS[family]; }
  protected assetFamily(asset: Asset): FormatFamily { return asset.kind === 'file' || asset.kind === 'archive' ? asset.data.format.family : 'unknown'; }
  protected assetFormat(asset: Asset): string { switch (asset.kind) { case 'directory': return 'Dossier'; case 'symlink': return 'Lien'; case 'archive': case 'file': return asset.data.format.id; } }
  protected assetMark(asset: Asset): string { switch (asset.kind) { case 'directory': return 'DIR'; case 'symlink': return 'LNK'; case 'archive': return 'ZIP'; case 'file': return (asset.data.format.extension ?? asset.data.format.id).slice(0,4); } }
  protected assetSize(asset: Asset): string { return asset.kind === 'file' || asset.kind === 'archive' ? this.formatBytes(asset.data.sizeBytes) : '—'; }
  protected actionMark(action: ActionDescriptor): string { return ACTION_MARKS[action.category] ?? action.title.slice(0,2).toUpperCase(); }
  protected archiveSamplePaths(archive: ArchiveInspection): string { return archive.samples.slice(0, 3).map((sample) => sample.path).join(' · '); }
  protected formatBytes(bytes: number): string { if (bytes < 1024) return `${bytes} o`; const units=['Ko','Mo','Go','To']; let value=bytes/1024,index=0; while(index<units.length-1&&value>=1024){value/=1024;index+=1;} return `${value>=10?value.toFixed(0):value.toFixed(1)} ${units[index]}`; }
}

const TARGET_OPTIONS: Record<string, string[]> = {
  'image-convert': ['jpg','png','webp','avif','heic','tiff','gif'],
  'image-batch-convert': ['jpg','png','webp','avif'],
  'audio-convert': ['mp3','m4a','wav','flac','ogg','opus'],
  'extract-audio': ['m4a','mp3','wav','flac'],
  'archive-create': ['zip','7z','tar'],
};
const QUALITY_ACTIONS = new Set(['image-optimize','image-resize','pdf-compress','media-compress']);
function defaultTarget(action: ActionDescriptor): string | null {
  if (action.outputFormat) return action.outputFormat;
  return TARGET_OPTIONS[action.id]?.[0] ?? null;
}

const FAMILY_LABELS: Record<FormatFamily,string> = { image:'Images',pdf:'PDF',document:'Documents',spreadsheet:'Tableurs',presentation:'Présentations',audio:'Audio',video:'Vidéos',archive:'Archives',ebook:'Livres',text:'Texte',unknown:'Autres' };
const ACTION_MARKS: Record<string,string> = { pdf:'PDF',image:'IMG',media:'▶',archive:'ZIP',extract:'Aa',organize:'▦',privacy:'◌',optimize:'↓',convert:'↔',document:'DOC' };
