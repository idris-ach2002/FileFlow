import { ChangeDetectionStrategy, Component, computed, effect, inject, signal } from '@angular/core';
import { DomSanitizer, SafeResourceUrl } from '@angular/platform-browser';
import { Router } from '@angular/router';
import { convertFileSrc } from '@tauri-apps/api/core';
import { AuthStore } from '../../core/auth/auth.store';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor, Asset, DestinationPolicy, FormatFamily } from '../../core/ipc/tauri.models';
import { PreferencesService } from '../../core/preferences/preferences.service';
import { UiMemoryService } from '../../core/state/ui-memory.service';
import { WorkspaceStore } from './data-access/workspace.store';

type SimpleTask = 'convert' | 'compress' | 'extract' | 'organize' | 'rename' | 'protect';
type Quality = 'small' | 'balanced' | 'high';
type CompressionProfile = 'keep' | 'small' | 'balanced' | 'high';
type WorkspaceDestination = 'subfolder' | 'same' | 'choose';

export function resolveWorkspaceDestination(
  selection: WorkspaceDestination,
  explicitDirectory: string | null,
  guidedDirectory: string | null,
): { destination: DestinationPolicy; customDirectory: string | null } {
  const chosen = selection === 'choose' ? explicitDirectory?.trim() || null : null;
  if (chosen) return { destination: 'customFolder', customDirectory: chosen };
  if (selection === 'same') return { destination: 'sameFolder', customDirectory: null };
  const guided = selection === 'subfolder' ? guidedDirectory?.trim() || null : null;
  return guided
    ? { destination: 'customFolder', customDirectory: guided }
    : { destination: 'subfolder', customDirectory: null };
}

interface TaskCard {
  id: SimpleTask;
  title: string;
  description: string;
  icon: string;
  tone: string;
}

const TASKS: TaskCard[] = [
  { id: 'convert', title: 'Convertir', description: 'Changer le format du fichier', icon: '↻', tone: 'blue' },
  { id: 'compress', title: 'Compresser', description: 'Réduire la taille du fichier', icon: '↓', tone: 'green' },
  { id: 'extract', title: 'Extraire le texte', description: 'Récupérer le texte du document', icon: 'T', tone: 'violet' },
  { id: 'organize', title: 'Organiser', description: 'Fusionner, diviser ou réorganiser', icon: '▱', tone: 'orange' },
  { id: 'rename', title: 'Renommer', description: 'Changer le nom proprement', icon: '✎', tone: 'cyan' },
  { id: 'protect', title: 'Protéger', description: 'Mot de passe ou vie privée', icon: '▣', tone: 'red' },
];

@Component({
  selector: 'ff-workspace-page',
  template: `
    <div class="guided-shell">
      <nav class="flow-steps" aria-label="Progression">
        @for (step of [1,2,3,4,5]; track step) {
          <div class="flow-step" [class.active]="currentStep() === step" [class.done]="currentStep() > step">
            <span>{{ currentStep() > step ? '✓' : step }}</span>
            <strong>{{ stepLabel(step) }}</strong>
          </div>
          @if (step < 5) { <i aria-hidden="true">→</i> }
        }
      </nav>

      @if (store.error()) {
        <section class="state-card error-state">
          <span>!</span><div><h2>FileFlow n’a pas pu analyser cette sélection.</h2><p>{{ store.error() }}</p></div>
          <button class="ff-button" type="button" (click)="backHome()">Recommencer</button>
        </section>
      } @else if (!store.hasWorkspace()) {
        <section class="state-card loading-state">
          <span class="spinner"></span><div><h2>Récupération de votre espace…</h2><p>FileFlow reprend votre dernière sélection quand elle est encore disponible.</p></div>
        </section>
      } @else if (store.busy()) {
        <section class="stage-card treatment-card">
          <div class="stage-number">2</div>
          <div class="processing-orb"><span>✦</span></div>
          <h1>Je regarde vos fichiers…</h1>
          <p>{{ store.stats().discovered }} élément(s) détecté(s) · {{ formatBytes(store.stats().totalBytes) }}</p>
          <div class="progress indeterminate"><span></span></div>
          <div class="privacy-note">◆ Vos fichiers restent sur cet appareil.</div>
        </section>
      } @else {
        @switch (currentStep()) {
          @case (2) {
            <section class="stage-card action-stage">
              <header class="stage-header">
                <div><div class="stage-number">2</div><p class="kicker">APRÈS AJOUT DU FICHIER</p><h1>Que voulez-vous faire<br />avec {{ selectionLabel() }}&nbsp;?</h1></div>
                <button class="quiet-button" type="button" (click)="backHome()">＋ Autres fichiers</button>
              </header>

              <div class="source-strip">
                @for (asset of visibleSourceAssets(); track asset.data.id) {
                  <button type="button" class="source-preview-card" (click)="openAssetPreview(asset)"><span class="file-badge" [attr.data-family]="assetFamily(asset)">{{ assetMark(asset) }}</span><span><strong>{{ asset.data.name }}</strong><small>{{ assetFormat(asset) }} · {{ assetSize(asset) }}</small></span><b>↗</b></button>
                }
                @if (sourceOverflow() > 0) { <div class="more-files">+ {{ sourceOverflow() }} autre{{ sourceOverflow() > 1 ? 's' : '' }}</div> }
              </div>

              <div class="task-grid">
                @for (task of tasks; track task.id) {
                  <button type="button" class="task-card" [attr.data-tone]="task.tone" (click)="chooseTask(task.id)">
                    <span class="task-icon">{{ task.icon }}</span>
                    <span><strong>{{ task.title }}</strong><small>{{ task.description }}</small></span>
                    <b>›</b>
                  </button>
                }
              </div>

              <div class="smart-suggestion">
                <span>✦</span>
                <div><strong>{{ smartSuggestionTitle() }}</strong><small>{{ smartSuggestionText() }}</small></div>
                <button type="button" (click)="chooseTask(smartSuggestedTask())">Utiliser</button>
              </div>
            </section>
          }
          @case (3) {
            <section class="stage-card configure-stage">
              <header class="stage-header compact">
                <div><div class="stage-number">3</div><p class="kicker">CONFIGURER</p><h1>{{ configureTitle() }}</h1><p>{{ configureSubtitle() }}</p></div>
                <button class="quiet-button" type="button" (click)="returnToActions()">← Changer d’action</button>
              </header>

              <div class="configure-layout">
                <div class="essential-settings">
                  <h2>Réglages essentiels</h2>

                  @if (selectedTask() === 'convert' || selectedTask() === 'organize') {
                    <label class="setting-card">
                      <span class="setting-icon">◎</span>
                      <span><strong>Format cible</strong><small>FileFlow choisit le chemin le plus fiable.</small></span>
                      <select [value]="targetFormat()" (change)="setTarget($any($event.target).value)">
                        @for (format of targetOptions(); track format.value) { <option [value]="format.value">{{ format.label }}</option> }
                      </select>
                    </label>
                  }

                  @if (supportsQuality()) {
                    <label class="setting-card">
                      <span class="setting-icon">▥</span>
                      <span><strong>Qualité</strong><small>Équilibrée est recommandée dans la plupart des cas.</small></span>
                      <select [value]="quality()" (change)="quality.set($any($event.target).value)">
                        <option value="small">Petite taille</option><option value="balanced">Équilibrée (recommandée)</option><option value="high">Haute qualité</option>
                      </select>
                    </label>
                  }

                  <div class="setting-card destination-setting">
                    <span class="setting-icon">▱</span>
                    <span><strong>Destination</strong><small>{{ destinationLabel() }}</small></span>
                    <button type="button" (click)="chooseDestination()">Changer…</button>
                  </div>

                  @if (selectedTask() === 'organize' && isSinglePdf()) {
                    <label class="setting-card">
                      <span class="setting-icon">⌁</span>
                      <span><strong>Découpage</strong><small>Un fichier PDF sera créé par page.</small></span>
                      <span class="read-only-value">1 PDF / page</span>
                    </label>
                  }

                  @if (selectedTask() === 'organize' && !isSinglePdf()) {
                    <label class="setting-card">
                      <span class="setting-icon">☷</span>
                      <span><strong>Ordre des fichiers</strong><small>Détermine l’ordre des documents dans le PDF final.</small></span>
                      <select [value]="collectionOrder()" (change)="collectionOrder.set($any($event.target).value)">
                        <option value="name">Alphabétique (recommandé)</option>
                        <option value="date">Date de modification</option>
                        <option value="selection">Ordre de sélection</option>
                      </select>
                    </label>
                  }

                  @if (selectedTask() === 'protect') {
                    <label class="setting-card password-setting">
                      <span class="setting-icon">▣</span>
                      <span><strong>Mot de passe du PDF</strong><small>Il n’est jamais mémorisé par FileFlow.</small></span>
                      <span class="password-wrap"><input [type]="showPdfPassword() ? 'text' : 'password'" autocomplete="new-password" [value]="pdfPassword()" (input)="pdfPassword.set($any($event.target).value)" /><button type="button" (mousedown)="$event.preventDefault()" (click)="showPdfPassword.update(v=>!v)" [attr.aria-label]="showPdfPassword() ? 'Masquer le mot de passe' : 'Afficher le mot de passe'">{{ showPdfPassword() ? '◉' : '◌' }}</button></span>
                    </label>
                  }

                  <button class="advanced-toggle" type="button" (click)="advancedOpen.update(v=>!v)"><span>⚙</span><strong>Options avancées</strong><small>sur demande</small><b>{{ advancedOpen() ? '−' : '+' }}</b></button>

                  @if (advancedOpen()) {
                    <div class="advanced-options">
                      @if (willProducePdf()) {
                        <div class="advanced-group">
                          <div><strong>Finalisation PDF</strong><small>Ces réglages sont appliqués après la conversion et avant le résultat final.</small></div>
                          <label><span>Compression finale</span><select [value]="finalCompression()" (change)="finalCompression.set($any($event.target).value)"><option value="keep">Conserver</option><option value="small">Plus léger</option><option value="balanced">Équilibré</option><option value="high">Haute qualité</option></select></label>
                          <label><span>Taille cible (facultatif)</span><div class="suffix-input"><input type="number" min="0" max="4096" step="1" [value]="targetSizeMb() ?? ''" (input)="setTargetSize($any($event.target).value)" placeholder="ex. 5" /><small>Mo</small></div></label>
                          <label class="check-line"><input type="checkbox" [checked]="improve()" (change)="improve.set($any($event.target).checked)" /><span><strong>Améliorer les scans</strong><small>Redressement + OCR lorsque le moteur le permet.</small></span></label>
                          <label class="check-line"><input type="checkbox" [checked]="stripMetadata()" (change)="stripMetadata.set($any($event.target).checked)" /><span><strong>Nettoyer les métadonnées</strong><small>Utile avant un partage.</small></span></label>
                        </div>
                        <div class="advanced-group signature-group">
                          <div><strong>Signature</strong><small>La V1 ajoute une page de signature visuelle sans rasteriser le document. La signature cryptographique reste réservée au mode expert.</small></div>
                          <label><span>Nom / signature visuelle</span><input type="text" maxlength="180" [value]="signatureText()" (input)="signatureText.set($any($event.target).value)" placeholder="Votre nom ou signature" /></label>
                        </div>
                      }
                      <div class="route-info">
                        <span>⇢</span><div><strong>{{ routeSummary() }}</strong><small>{{ routeDetail() }}</small></div>
                      </div>
                    </div>
                  }

                  <button class="launch-button" type="button" [disabled]="!canRun()" (click)="runCurrent()"><span>▶</span>{{ launchLabel() }}</button>
                  @if (store.executionError()) { <div class="inline-error">{{ store.executionError() }}</div> }
                </div>

                <aside class="preview-panel">
                  <div class="preview-head"><span>APERÇU</span><strong>{{ previewTitle() }}</strong></div>
                  @if (primaryFamily() === 'archive') {
                    <button class="archive-preview-card" type="button" (click)="openArchiveBrowser()">
                      <span class="archive-preview-icon">ZIP</span>
                      <strong>{{ primaryFileName() }}</strong>
                      <small>Afficher le contenu sous forme de cartes paginées</small>
                      <b>Voir le contenu →</b>
                    </button>
                  } @else if (pdfPreviewUrl(); as url) {
                    <button class="preview-clickable" type="button" (click)="openSourcePreview()"><iframe [src]="url" title="Aperçu du PDF" tabindex="-1"></iframe><span>Agrandir</span></button>
                  } @else if (imagePreviewUrl(); as image) {
                    <button class="image-preview preview-clickable" type="button" (click)="openSourcePreview()"><img [src]="image" alt="Aperçu du fichier" /><span>Agrandir</span></button>
                  } @else if (previewLoading()) {
                    <div class="preview-placeholder preview-loading"><span class="spinner"></span><strong>Préparation de l’aperçu…</strong><small>FileFlow crée localement une représentation compatible de ce format.</small></div>
                  } @else {
                    <div class="preview-placeholder"><span>{{ primaryFileMark() }}</span><strong>{{ primaryFileName() }}</strong><small>{{ previewError() || 'Ce format ne possède pas de représentation visuelle fiable.' }}</small><button type="button" class="quiet-button" (click)="retryPreview()">Réessayer l’aperçu</button></div>
                  }
                </aside>
              </div>
            </section>
          }
          @case (4) {
            <section class="stage-card treatment-card">
              <div class="stage-number">4</div>
              <div class="processing-orb"><span>⚙</span></div>
              <h1>Traitement en cours…</h1>
              <p>{{ processingLabel() }}</p>
              <div class="progress"><span [style.width.%]="Math.max(8, store.executionProgress())"></span><b>{{ store.executionProgress() }} %</b></div>
              <div class="timeline">
                <div [class.done]="phaseRank() > 0" [class.active]="phaseRank() === 0"><span>{{ phaseRank() > 0 ? '✓' : '' }}</span><div><strong>Préparation</strong><small>Analyse, extraction sûre et choix de la route</small></div></div>
                <div [class.done]="phaseRank() > 1" [class.active]="phaseRank() === 1"><span>{{ phaseRank() > 1 ? '✓' : '' }}</span><div><strong>Conversion</strong><small>{{ phaseDetail() }}</small></div></div>
                <div [class.done]="phaseRank() > 2" [class.active]="phaseRank() === 2"><span>{{ phaseRank() > 2 ? '✓' : '' }}</span><div><strong>Assemblage</strong><small>Fusion des pages dans l’ordre choisi</small></div></div>
                <div [class.done]="phaseRank() > 3" [class.active]="phaseRank() >= 3"><span>{{ phaseRank() > 3 ? '✓' : '' }}</span><div><strong>Finalisation</strong><small>Qualité, validation puis nettoyage des intermédiaires</small></div></div>
              </div>
              <button class="cancel-button" type="button" (click)="store.cancelExecution()">Annuler</button>
              <div class="privacy-note">◇ Les fichiers intermédiaires sont supprimés après validation du résultat.</div>
            </section>
          }
          @case (5) {
            <section class="stage-card result-card" [class.failed]="store.executionSummary()?.state !== 'completed'">
              @if (store.executionSummary()?.state === 'completed') {
                <div class="stage-number">5</div>
                <div class="success-orb">✓</div>
                <h1>C’est prêt&nbsp;!</h1>
                <p>Votre fichier a été traité avec succès.</p>
                <div class="result-layout">
                  <button class="result-preview-card" type="button" (click)="openResultPreview()">
                    @if (resultPdfPreviewUrl(); as resultPdf) {
                      <iframe [src]="resultPdf" title="Aperçu du résultat" tabindex="-1"></iframe>
                    } @else if (resultImagePreviewUrl(); as resultImage) {
                      <img [src]="resultImage" alt="Aperçu du résultat" />
                    } @else {
                      <span class="result-preview-mark">{{ resultMark(store.executionSummary()?.outputs?.[0] ?? '') }}</span>
                    }
                    <b>Prévisualiser en grand</b>
                  </button>
                  <div class="result-actions">
                    @for (output of store.executionSummary()?.outputs ?? []; track output; let index = $index) {
                      <div class="result-file"><span>{{ resultMark(output) }}</span><div><strong>{{ fileName(output) }}</strong><small>{{ extensionLabel(output) }}</small></div><button type="button" (click)="store.openOutput(index)">↗</button></div>
                    }
                    <button class="open-result" type="button" (click)="store.openOutput(0)">Ouvrir <span>↗</span></button>
                    <button class="secondary-result" type="button" (click)="store.revealOutput(0)"><span>▱</span> Afficher dans le dossier</button>
                    <button class="secondary-result" type="button" (click)="restart()"><span>↻</span> Recommencer</button>
                  </div>
                </div>
                <div class="cleanup-note"><span>✓</span> Espace temporaire nettoyé après validation.</div>
              } @else {
                <div class="stage-number">5</div><div class="failure-orb">!</div><h1>Le traitement n’a pas abouti.</h1><p>{{ store.executionError() || store.executionFailures()[0] || 'FileFlow a arrêté le traitement sans modifier vos originaux.' }}</p><button class="open-result" type="button" (click)="restart()">Réessayer</button><button class="secondary-result" type="button" (click)="returnToActions()">Changer d’action</button>
              }
            </section>
          }
        }
      }

      @if (archiveBrowserOpen() && !fullscreenPreviewPath()) {
        <div class="viewer-backdrop" (click)="closeArchiveBrowser()">
          <section class="archive-browser" role="dialog" aria-modal="true" aria-label="Contenu de l’archive" (click)="$event.stopPropagation()">
            <header><div><span>CONTENU DE L’ARCHIVE</span><h2>{{ archiveBrowserTitle() }}</h2><p>{{ store.archiveInspection()?.files ?? 0 }} fichier(s) · {{ formatBytes(store.archiveInspection()?.totalUnpackedBytes ?? 0) }}</p></div><button type="button" (click)="closeArchiveBrowser()" aria-label="Fermer">×</button></header>
            @if (store.archiveInspectionLoading()) { <div class="viewer-loading"><span class="spinner"></span>Lecture de l’archive…</div> }
            @else if (store.archiveInspectionError()) { <div class="viewer-error">{{ store.archiveInspectionError() }}</div> }
            @else {
              @if (archiveEntryPreviewLoading()) { <div class="archive-preview-progress"><span class="spinner"></span>Préparation de {{ archiveEntryPreviewName() }}…</div> }
              <div class="archive-card-grid">
                @for (entry of store.archiveInspection()?.samples ?? []; track entry.path) {
                  <button type="button" class="archive-entry-card" [disabled]="archiveEntryPreviewLoading()" (click)="previewArchiveEntry(entry.path)">
                    <span [attr.data-family]="entry.family">{{ familyMark(entry.family) }}</span>
                    <strong>{{ archiveEntryName(entry.path) }}</strong>
                    <small>{{ entry.family }} · {{ formatBytes(entry.sizeBytes) }}</small>
                    <em>{{ entry.path }}</em>
                  </button>
                }
              </div>
              <footer class="archive-pagination"><button type="button" [disabled]="archivePage() === 0" (click)="archivePrevious()">← Précédent</button><span>Page {{ archivePage()+1 }}</span><button type="button" [disabled]="!store.archiveInspection()?.hasMore" (click)="archiveNext()">Suivant →</button></footer>
            }
          </section>
        </div>
      }

      @if (fullscreenPreviewPath()) {
        <div class="viewer-backdrop" (click)="closeFullscreenPreview()">
          <section class="fullscreen-viewer" role="dialog" aria-modal="true" aria-label="Prévisualisation" (click)="$event.stopPropagation()">
            <header><div><span>PRÉVISUALISATION</span><strong>{{ fullscreenPreviewTitle() }}</strong></div><button type="button" (click)="closeFullscreenPreview()" aria-label="Fermer">×</button></header>
            <div class="fullscreen-preview-content">
              @if (fullscreenPdfUrl(); as fullPdf) { <iframe [src]="fullPdf" title="Prévisualisation PDF"></iframe> }
              @else if (fullscreenImageUrl(); as fullImage) { <img [src]="fullImage" alt="Prévisualisation du fichier" /> }
              @else { <div class="preview-placeholder"><span>{{ fullscreenPreviewMark() }}</span><strong>{{ fullscreenPreviewTitle() }}</strong><small>Ce format ne possède pas encore de rendu intégré. Vous pouvez toujours l’ouvrir avec son application système.</small></div> }
            </div>
          </section>
        </div>
      }
    </div>
  `,
  styles: [`
    :host{display:block}.guided-shell{max-width:1120px;margin:0 auto;padding:4px 0 32px}.flow-steps{display:grid;grid-template-columns:auto 20px auto 20px auto 20px auto 20px auto;align-items:center;justify-content:center;gap:7px;margin:3px auto 25px}.flow-step{display:flex;align-items:center;gap:7px;color:var(--text-faint);font-size:10px;font-weight:760}.flow-step span{width:25px;height:25px;display:grid;place-items:center;border-radius:50%;background:var(--surface-2);border:1px solid var(--border);font-size:10px}.flow-step.active{color:var(--text)}.flow-step.active span{background:var(--accent);border-color:var(--accent);color:white;box-shadow:0 5px 16px color-mix(in srgb,var(--accent) 25%,transparent)}.flow-step.done{color:var(--success)}.flow-step.done span{background:var(--success-soft);border-color:transparent}.flow-steps>i{color:var(--border-strong);font-style:normal}.stage-card,.state-card{position:relative;border:1px solid var(--border);border-radius:28px;background:var(--surface-1);box-shadow:var(--shadow-sm)}.stage-number{width:31px;height:31px;display:grid;place-items:center;border-radius:50%;background:var(--accent-soft);color:var(--accent);font-size:13px;font-weight:900}.stage-header{display:flex;justify-content:space-between;align-items:flex-start;gap:24px}.stage-header>div>.stage-number{display:inline-grid;margin-right:10px;vertical-align:middle}.stage-header .kicker{display:inline-block;margin:0;color:var(--text-faint);font-size:10px;font-weight:900;letter-spacing:.09em}.stage-header h1{margin:14px 0 0;font-size:42px;line-height:1.02;letter-spacing:-.055em}.stage-header p:last-child{max-width:650px;margin:9px 0 0;color:var(--text-muted);font-size:13px;line-height:1.55}.stage-header.compact h1{font-size:38px}.quiet-button{min-height:38px;padding:0 13px;border:1px solid var(--border);border-radius:11px;background:var(--surface-2);color:var(--text-muted);font-size:11px;font-weight:750}.quiet-button:hover{border-color:var(--border-strong);color:var(--text)}.action-stage,.configure-stage{padding:27px}.source-strip{display:flex;align-items:center;gap:8px;margin:24px 0 20px;padding:9px;border:1px solid var(--border);border-radius:15px;background:var(--surface-2);overflow:hidden}.source-strip article,.source-strip .source-preview-card{min-width:0;display:grid;grid-template-columns:38px minmax(0,1fr);align-items:center;gap:9px;padding:4px 7px}.source-strip article>div,.source-strip .source-preview-card>span:nth-child(2){min-width:0}.source-strip strong,.source-strip small{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.source-strip strong{font-size:11px}.source-strip small{margin-top:2px;color:var(--text-muted);font-size:9px}.file-badge{width:36px;height:36px;display:grid;place-items:center;border-radius:10px;background:var(--accent-soft);color:var(--accent);font-size:9px;font-weight:900}.file-badge[data-family=pdf]{background:var(--danger-soft);color:var(--danger)}.file-badge[data-family=image]{background:var(--success-soft);color:var(--success)}.more-files{margin-left:auto;flex:none;padding:6px 9px;border-radius:999px;background:var(--surface-1);color:var(--text-muted);font-size:10px;font-weight:800}.source-preview-card{border:0;background:transparent;color:var(--text);text-align:left}.source-preview-card>b{color:var(--accent);font-size:12px}.source-preview-card:hover{background:var(--surface-1);border-radius:11px}.task-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px}.task-card{min-height:105px;display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:13px;padding:13px 15px;border:1px solid var(--border);border-radius:17px;background:var(--surface-1);color:var(--text);text-align:left;transition:var(--transition)}.task-card:hover{transform:translateY(-2px);border-color:color-mix(in srgb,var(--accent) 24%,var(--border));box-shadow:var(--shadow-md)}.task-icon{width:45px;height:45px;display:grid;place-items:center;border-radius:13px;background:var(--accent-soft);color:var(--accent);font-size:21px;font-weight:800}.task-card[data-tone=green] .task-icon{background:var(--success-soft);color:var(--success)}.task-card[data-tone=violet] .task-icon{background:color-mix(in srgb,var(--violet) 12%,var(--surface-1));color:var(--violet)}.task-card[data-tone=orange] .task-icon{background:var(--warning-soft);color:var(--warning)}.task-card[data-tone=cyan] .task-icon{background:color-mix(in srgb,#16a7c8 12%,var(--surface-1));color:#1688a4}.task-card[data-tone=red] .task-icon{background:var(--danger-soft);color:var(--danger)}.task-card strong,.task-card small{display:block}.task-card strong{font-size:15px}.task-card small{margin-top:4px;color:var(--text-muted);font-size:11px}.task-card>b{color:var(--text-faint);font-size:23px}.smart-suggestion{display:grid;grid-template-columns:38px minmax(0,1fr) auto;align-items:center;gap:10px;margin-top:12px;padding:12px 14px;border-radius:15px;background:var(--accent-soft);color:var(--accent-strong)}.smart-suggestion>span{font-size:20px}.smart-suggestion strong,.smart-suggestion small{display:block}.smart-suggestion strong{font-size:11px}.smart-suggestion small{margin-top:2px;color:var(--text-muted);font-size:10px}.smart-suggestion button{padding:7px 10px;border:0;border-radius:9px;background:var(--surface-1);color:var(--accent);font-size:10px;font-weight:850}.configure-layout{display:grid;grid-template-columns:minmax(0,1.02fr) minmax(330px,.98fr);gap:15px;margin-top:22px}.essential-settings,.preview-panel{padding:18px;border:1px solid var(--border);border-radius:20px;background:var(--surface-2)}.essential-settings>h2{margin:0 0 12px;font-size:14px}.setting-card{min-height:78px;display:grid;grid-template-columns:42px minmax(0,1fr) minmax(145px,210px);align-items:center;gap:11px;padding:10px 12px;border:1px solid var(--border);border-radius:14px;background:var(--surface-1);color:var(--text)}.setting-card+.setting-card{margin-top:8px}.setting-icon{width:38px;height:38px;display:grid;place-items:center;border-radius:11px;background:var(--accent-soft);color:var(--accent);font-size:16px}.setting-card strong,.setting-card small{display:block}.setting-card strong{font-size:12px}.setting-card small{margin-top:3px;color:var(--text-muted);font-size:9.5px;line-height:1.4}.setting-card select,.setting-card input,.advanced-options select,.advanced-options input{width:100%;height:38px;padding:0 10px;border:1px solid var(--border-strong);border-radius:10px;background:var(--surface-2);color:var(--text);font:inherit;font-size:11px;outline:none}.setting-card select:focus,.setting-card input:focus,.advanced-options select:focus,.advanced-options input:focus{border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--accent) 12%,transparent)}.destination-setting button{height:37px;border:1px solid var(--border);border-radius:10px;background:var(--surface-2);color:var(--text);font-size:10px;font-weight:780}.read-only-value{justify-self:end;color:var(--text-muted);font-size:10px;font-weight:800}.password-wrap{position:relative}.password-wrap input{padding-right:38px}.password-wrap button{position:absolute;right:3px;top:3px;width:32px;height:32px;border:0;border-radius:8px;background:transparent;color:var(--text-muted);font-size:16px}.advanced-toggle{width:100%;min-height:50px;display:grid;grid-template-columns:28px minmax(0,1fr) auto auto;align-items:center;gap:7px;margin-top:10px;padding:0 11px;border:0;border-radius:12px;background:transparent;color:var(--accent);text-align:left}.advanced-toggle:hover{background:var(--accent-soft)}.advanced-toggle strong{font-size:11px}.advanced-toggle small{color:var(--text-faint);font-size:9px}.advanced-toggle b{font-size:17px}.advanced-options{display:grid;gap:9px;margin-top:4px;padding:12px;border:1px dashed color-mix(in srgb,var(--accent) 24%,var(--border));border-radius:14px;background:color-mix(in srgb,var(--accent-soft) 35%,var(--surface-1))}.advanced-group{display:grid;grid-template-columns:1fr 1fr;gap:9px;padding-bottom:10px;border-bottom:1px solid var(--border)}.advanced-group>div{grid-column:1/-1}.advanced-group strong,.advanced-group small{display:block}.advanced-group strong{font-size:11px}.advanced-group small{margin-top:3px;color:var(--text-muted);font-size:9px;line-height:1.45}.advanced-group label{display:grid;gap:4px;color:var(--text-muted);font-size:9px;font-weight:750}.check-line{grid-template-columns:auto 1fr!important;align-items:start}.check-line input{width:15px!important;height:15px!important;margin-top:2px;accent-color:var(--accent)}.suffix-input{position:relative}.suffix-input input{padding-right:34px}.suffix-input small{position:absolute;right:10px;top:11px}.signature-group{grid-template-columns:1fr}.route-info{display:grid;grid-template-columns:28px 1fr;gap:8px;align-items:center;padding:9px;border-radius:10px;background:var(--surface-1)}.route-info>span{color:var(--accent);font-size:18px}.route-info strong,.route-info small{display:block}.route-info strong{font-size:10px}.route-info small{margin-top:2px;color:var(--text-muted);font-size:9px}.launch-button{width:100%;min-height:50px;display:flex;align-items:center;justify-content:center;gap:9px;margin-top:13px;border:0;border-radius:13px;background:linear-gradient(135deg,var(--accent),var(--violet));color:white;font-size:13px;font-weight:850;box-shadow:0 12px 28px color-mix(in srgb,var(--accent) 22%,transparent)}.launch-button:disabled{opacity:.42;box-shadow:none}.inline-error{margin-top:8px;padding:9px 10px;border-radius:9px;background:var(--danger-soft);color:var(--danger);font-size:10px}.preview-panel{display:flex;flex-direction:column;min-height:520px;background:var(--surface-1)}.preview-head{display:flex;justify-content:space-between;align-items:center;padding-bottom:10px;border-bottom:1px solid var(--border)}.preview-head span{color:var(--text-faint);font-size:9px;font-weight:900;letter-spacing:.08em}.preview-head strong{font-size:11px}.preview-panel iframe{width:100%;flex:1;min-height:390px;margin-top:11px;border:1px solid var(--border);border-radius:12px;background:white}.image-preview{flex:1;display:grid;place-items:center;margin-top:11px;overflow:hidden;border:1px solid var(--border);border-radius:12px;background:linear-gradient(45deg,var(--surface-2) 25%,transparent 25%),linear-gradient(-45deg,var(--surface-2) 25%,transparent 25%);background-size:18px 18px}.image-preview img{max-width:90%;max-height:420px;object-fit:contain}.preview-placeholder{flex:1;display:grid;place-items:center;align-content:center;text-align:center;padding:28px}.preview-placeholder>span{width:68px;height:68px;display:grid;place-items:center;border-radius:18px;background:var(--accent-soft);color:var(--accent);font-size:20px;font-weight:900}.preview-placeholder strong{margin-top:12px;font-size:13px}.preview-placeholder small{max-width:330px;margin-top:6px;color:var(--text-muted);font-size:10px;line-height:1.5}.preview-trust{display:grid;grid-template-columns:26px 1fr;gap:6px;margin-top:10px;padding:10px;border-radius:11px;background:var(--success-soft);color:var(--success)}.preview-trust strong,.preview-trust small{display:block}.preview-trust strong{font-size:10px}.preview-trust small{margin-top:2px;color:var(--text-muted);font-size:9px}.treatment-card,.result-card{max-width:610px;min-height:610px;margin:0 auto;padding:34px;display:flex;flex-direction:column;align-items:center;text-align:center}.processing-orb,.success-orb,.failure-orb{width:92px;height:92px;display:grid;place-items:center;margin:44px 0 18px;border-radius:50%;font-size:37px}.processing-orb{background:var(--accent-soft);color:var(--accent);animation:softPulse 1.4s infinite alternate}.success-orb{background:var(--success-soft);color:var(--success)}.failure-orb{background:var(--danger-soft);color:var(--danger)}@keyframes softPulse{to{transform:scale(.96);opacity:.72}}.treatment-card h1,.result-card h1{margin:0;font-size:32px;letter-spacing:-.045em}.treatment-card>p,.result-card>p{margin:7px 0 0;color:var(--text-muted);font-size:12px}.progress{position:relative;width:100%;height:9px;margin-top:28px;border-radius:99px;background:var(--surface-3)}.progress>span{display:block;height:100%;border-radius:inherit;background:linear-gradient(90deg,var(--accent),var(--violet));transition:width .2s}.progress>b{position:absolute;left:calc(100% + 9px);top:-4px;color:var(--text-muted);font-size:10px}.progress.indeterminate span{width:34%;animation:slide 1.2s infinite alternate}@keyframes slide{to{margin-left:66%}}.timeline{width:100%;display:grid;gap:15px;margin-top:32px;text-align:left}.timeline>div{display:grid;grid-template-columns:28px 1fr;gap:9px;align-items:center;color:var(--text-faint)}.timeline>div>span{width:24px;height:24px;display:grid;place-items:center;border:2px solid var(--border-strong);border-radius:50%;font-size:10px}.timeline .done{color:var(--success)}.timeline .done>span{border-color:var(--success);background:var(--success);color:white}.timeline .active{color:var(--accent)}.timeline .active>span{border-color:var(--accent);box-shadow:inset 0 0 0 5px var(--surface-1),0 0 0 3px var(--accent-soft);background:var(--accent)}.timeline strong,.timeline small{display:block}.timeline strong{font-size:11px}.timeline small{margin-top:2px;color:var(--text-muted);font-size:9px}.cancel-button{margin-top:24px;padding:8px 14px;border:1px solid var(--border);border-radius:10px;background:transparent;color:var(--text-muted);font-size:10px}.privacy-note{margin-top:auto;padding:10px 12px;border-radius:11px;background:var(--accent-soft);color:var(--text-muted);font-size:9px}.result-file{width:100%;display:grid;grid-template-columns:42px minmax(0,1fr) 34px;align-items:center;gap:10px;margin-top:24px;padding:11px;border:1px solid var(--border);border-radius:13px;background:var(--surface-2);text-align:left}.result-file>span{width:39px;height:39px;display:grid;place-items:center;border-radius:10px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:900}.result-file strong,.result-file small{display:block}.result-file strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px}.result-file small{margin-top:2px;color:var(--text-muted);font-size:9px}.result-file button{width:32px;height:32px;border:0;border-radius:9px;background:var(--surface-1);color:var(--accent)}.open-result,.secondary-result{width:100%;min-height:48px;margin-top:10px;border-radius:12px;font-size:11px;font-weight:820}.open-result{display:flex;align-items:center;justify-content:center;gap:8px;border:0;background:linear-gradient(135deg,var(--accent),var(--violet));color:white}.secondary-result{border:1px solid var(--border);background:var(--surface-1);color:var(--text)}.cleanup-note{margin-top:auto;color:var(--success);font-size:9px}.state-card{max-width:650px;margin:80px auto;padding:25px;display:grid;grid-template-columns:48px minmax(0,1fr) auto;align-items:center;gap:14px}.state-card>span{width:44px;height:44px;display:grid;place-items:center;border-radius:13px;background:var(--accent-soft);color:var(--accent)}.error-state>span{background:var(--danger-soft);color:var(--danger)}.state-card h2{margin:0;font-size:16px}.state-card p{margin:4px 0 0;color:var(--text-muted);font-size:10px}.spinner{border:3px solid var(--border)!important;border-top-color:var(--accent)!important;border-radius:50%!important;animation:spin .8s linear infinite}@keyframes spin{to{transform:rotate(1turn)}}
.preview-clickable{position:relative;width:100%;flex:1;min-height:0;margin-top:11px;padding:0;overflow:hidden;border:1px solid var(--border);border-radius:12px;background:var(--surface-1);cursor:zoom-in}.preview-clickable iframe{pointer-events:none;margin:0;border:0;border-radius:0}.preview-clickable>span{position:absolute;right:10px;bottom:10px;padding:6px 9px;border-radius:999px;background:rgb(20 24 39 / 76%);color:white;font-size:9px;font-weight:800;backdrop-filter:blur(7px)}.image-preview.preview-clickable{display:grid;place-items:center}.image-preview.preview-clickable img{max-width:100%;max-height:100%;object-fit:contain}.archive-preview-card{width:100%;flex:1;min-height:280px;display:grid;place-content:center;justify-items:center;gap:8px;margin-top:11px;padding:24px;border:1px dashed color-mix(in srgb,var(--accent) 35%,var(--border));border-radius:14px;background:linear-gradient(145deg,var(--surface-2),var(--accent-soft));color:var(--text);text-align:center}.archive-preview-icon{width:62px;height:62px;display:grid;place-items:center;border-radius:18px;background:var(--accent);color:white;font-size:12px;font-weight:900}.archive-preview-card strong{max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.archive-preview-card small{color:var(--text-muted);font-size:10px}.archive-preview-card b{margin-top:5px;color:var(--accent);font-size:10px}.result-layout{width:100%;display:grid;grid-template-columns:minmax(220px,.9fr) minmax(260px,1.1fr);gap:14px;margin-top:22px}.result-preview-card{min-height:270px;display:grid;place-items:center;position:relative;overflow:hidden;border:1px solid var(--border);border-radius:15px;background:var(--surface-2);color:var(--text);cursor:zoom-in}.result-preview-card iframe,.result-preview-card img{width:100%;height:100%;min-height:270px;border:0;object-fit:contain;pointer-events:none}.result-preview-card>b{position:absolute;bottom:10px;padding:6px 10px;border-radius:999px;background:rgb(20 24 39 / 76%);color:#fff;font-size:9px}.result-preview-mark{width:72px;height:72px;display:grid;place-items:center;border-radius:20px;background:var(--accent-soft);color:var(--accent);font-size:15px;font-weight:900}.result-actions{min-width:0}.viewer-backdrop{position:fixed;inset:0;z-index:1600;display:grid;place-items:center;padding:clamp(12px,3vw,36px);background:rgb(11 14 25 / 64%);backdrop-filter:blur(12px)}.fullscreen-viewer,.archive-browser{width:min(1180px,100%);max-height:calc(100vh - 40px);display:flex;flex-direction:column;overflow:hidden;border:1px solid var(--border-strong);border-radius:24px;background:var(--surface-1);box-shadow:var(--shadow-lg)}.fullscreen-viewer>header,.archive-browser>header{display:flex;align-items:center;justify-content:space-between;gap:14px;padding:15px 18px;border-bottom:1px solid var(--border)}.fullscreen-viewer header div,.archive-browser header div{min-width:0}.fullscreen-viewer header span,.archive-browser header span{display:block;color:var(--text-faint);font-size:9px;font-weight:900;letter-spacing:.08em}.fullscreen-viewer header strong,.archive-browser header h2{display:block;margin:3px 0 0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:15px}.archive-browser header p{margin:3px 0 0;color:var(--text-muted);font-size:10px}.fullscreen-viewer header button,.archive-browser header button{width:38px;height:38px;flex:none;border:0;border-radius:11px;background:var(--surface-2);color:var(--text);font-size:22px}.fullscreen-preview-content{min-height:0;flex:1;display:grid;place-items:center;overflow:auto;padding:14px;background:var(--surface-2)}.fullscreen-preview-content iframe{width:100%;height:min(76vh,850px);border:0;border-radius:12px;background:white}.fullscreen-preview-content img{max-width:100%;max-height:76vh;object-fit:contain;border-radius:10px}.archive-browser{height:min(820px,calc(100vh - 40px))}.archive-card-grid{min-height:0;flex:1;overflow:auto;display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));align-content:start;gap:10px;padding:14px}.archive-entry-card{min-width:0;min-height:150px;display:flex;flex-direction:column;align-items:flex-start;padding:13px;border:1px solid var(--border);border-radius:14px;background:var(--surface-2);color:var(--text);text-align:left}.archive-entry-card>span{width:38px;height:38px;display:grid;place-items:center;margin-bottom:11px;border-radius:11px;background:var(--accent-soft);color:var(--accent);font-size:9px;font-weight:900}.archive-entry-card strong{max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px}.archive-entry-card small{margin-top:3px;color:var(--text-muted);font-size:9px}.archive-entry-card em{max-width:100%;margin-top:auto;overflow:hidden;color:var(--text-faint);font-size:8px;font-style:normal;text-overflow:ellipsis;white-space:nowrap}.archive-pagination{display:flex;justify-content:center;align-items:center;gap:12px;padding:11px;border-top:1px solid var(--border)}.archive-pagination button{min-height:34px;padding:0 12px;border:1px solid var(--border);border-radius:10px;background:var(--surface-2);color:var(--text);font-size:10px}.archive-pagination button:disabled{opacity:.4}.archive-pagination span{color:var(--text-muted);font-size:10px}.viewer-loading,.viewer-error{min-height:260px;display:grid;place-items:center;align-content:center;gap:10px;color:var(--text-muted);font-size:11px}.viewer-error{color:var(--danger)}
@media(max-width:1180px){.guided-shell{max-width:none}.configure-layout{grid-template-columns:minmax(0,1fr) minmax(280px,.85fr)}.stage-header h1{font-size:clamp(32px,4vw,40px)}}
.archive-preview-progress{display:flex;align-items:center;justify-content:center;gap:8px;padding:9px 14px;border-bottom:1px solid var(--border);background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:800}.archive-preview-progress .spinner{width:16px;height:16px}.archive-entry-card:disabled{cursor:wait;opacity:.58}
@media(max-width:980px){.configure-layout{grid-template-columns:1fr}.preview-panel{min-height:390px}.result-layout{grid-template-columns:1fr}.result-preview-card{min-height:240px}.setting-card{grid-template-columns:42px minmax(0,1fr) minmax(130px,190px)}}
@media(max-width:900px){.flow-step strong{display:none}.flow-steps{grid-template-columns:auto 12px auto 12px auto 12px auto 12px auto}.task-grid{grid-template-columns:1fr}.advanced-group{grid-template-columns:1fr}.advanced-group>div{grid-column:auto}.source-strip{overflow:auto}.stage-card{border-radius:22px}}@media(max-width:620px){.guided-shell{padding-top:0}.flow-steps{width:100%;justify-content:space-between;gap:3px;margin-bottom:14px}.flow-steps>i{font-size:9px}.flow-step span{width:23px;height:23px}.action-stage,.configure-stage{padding:15px;border-radius:18px}.stage-header{display:block}.quiet-button{width:100%;margin-top:12px}.stage-header h1,.stage-header.compact h1{font-size:clamp(27px,9vw,32px)}.source-strip article:nth-child(n+2),.source-strip .source-preview-card:nth-child(n+2){display:none}.setting-card{grid-template-columns:38px minmax(0,1fr);padding:10px}.setting-card>select,.setting-card>button,.setting-card>.password-wrap,.setting-card>.read-only-value{grid-column:1/-1}.essential-settings,.preview-panel{padding:12px;border-radius:16px}.preview-panel{min-height:320px}.task-card{min-height:88px;grid-template-columns:42px minmax(0,1fr) auto;padding:10px}.task-icon{width:40px;height:40px}.treatment-card,.result-card{min-height:0;padding:20px 15px}.treatment-card{max-width:none}.processing-orb,.success-orb,.failure-orb{transform:scale(.82)}.timeline{width:100%}.result-layout{gap:10px}.result-preview-card{min-height:200px}.viewer-backdrop{padding:0}.fullscreen-viewer,.archive-browser{width:100%;height:100%;max-height:none;border:0;border-radius:0}.fullscreen-preview-content iframe{height:calc(100vh - 74px)}.archive-card-grid{grid-template-columns:repeat(2,minmax(0,1fr));padding:10px}.archive-entry-card{min-height:130px;padding:10px}.smart-suggestion{grid-template-columns:32px minmax(0,1fr)}.smart-suggestion button{grid-column:1/-1;width:100%}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WorkspacePage {
  protected readonly Math = Math;
  protected readonly store = inject(WorkspaceStore);
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly prefs = inject(PreferencesService);
  private readonly auth = inject(AuthStore);
  private readonly router = inject(Router);
  private readonly memory = inject(UiMemoryService);
  private readonly sanitizer = inject(DomSanitizer);

  protected readonly tasks = TASKS;
  protected readonly selectedTask = signal<SimpleTask | null>(null);
  protected readonly targetFormat = signal('pdf');
  protected readonly quality = signal<Quality>('balanced');
  protected readonly destination = signal<WorkspaceDestination>('subfolder');
  protected readonly customDirectory = signal<string | null>(null);
  protected readonly finalCompression = signal<CompressionProfile>('balanced');
  protected readonly improve = signal(false);
  protected readonly stripMetadata = signal(false);
  protected readonly targetSizeMb = signal<number | null>(null);
  protected readonly signatureText = signal('');
  protected readonly collectionOrder = signal<'name' | 'date' | 'selection'>('name');
  protected readonly advancedOpen = signal(false);
  protected readonly pdfPassword = signal('');
  protected readonly showPdfPassword = signal(false);
  protected readonly archiveBrowserOpen = signal(false);
  protected readonly archiveBrowserTitle = signal('Archive');
  protected readonly archivePage = signal(0);
  protected readonly archivePageSize = 24;
  protected readonly archiveEntryPreviewLoading = signal(false);
  protected readonly archiveEntryPreviewName = signal('ce fichier');
  protected readonly fullscreenPreviewPath = signal<string | null>(null);
  protected readonly fullscreenPreviewTitle = signal('Aperçu');
  protected readonly fullscreenPreviewFamily = signal<FormatFamily>('unknown');
  protected readonly preparedPreviewPath = signal<string | null>(null);
  protected readonly preparedPreviewFamily = signal<FormatFamily>('unknown');
  protected readonly previewLoading = signal(false);
  protected readonly previewError = signal<string | null>(null);
  private previewRequest = 0;
  private archivePreviewRequest = 0;

  protected readonly firstAsset = computed(() => this.store.selectedAssets()[0] ?? this.store.assets().find(a => a.kind === 'file' || a.kind === 'archive') ?? null);
  protected readonly primaryFamily = computed<FormatFamily>(() => this.firstAsset() ? this.assetFamily(this.firstAsset()!) : 'unknown');
  protected readonly selectionCount = computed(() => this.store.selectedCount() || this.store.counts().files + this.store.counts().archives);
  protected readonly isSinglePdf = computed(() => this.selectionCount() === 1 && this.primaryFamily() === 'pdf');
  protected readonly currentStep = computed(() => {
    if (this.store.executionSummary()) return 5;
    if (this.store.executing()) return 4;
    if (this.selectedTask() || this.store.activeActionId()) return 3;
    return 2;
  });
  protected readonly targetOptions = computed(() => this.computeTargetOptions());
  protected readonly resolvedActionId = computed(() => this.resolveActionId());
  protected readonly activeAction = computed<ActionDescriptor | null>(() => this.capabilities.action(this.resolvedActionId()));
  protected readonly willProducePdf = computed(() => this.targetFormat() === 'pdf' || ['smart-to-pdf','collection-to-pdf','pdf-compress','pdf-protect','pdf-merge','pdf-split','office-to-pdf','text-to-pdf','html-to-pdf','email-to-pdf','pdf-ocr'].includes(this.resolvedActionId() ?? ''));
  protected readonly pdfPreviewUrl = computed<SafeResourceUrl | null>(() => {
    const path = this.preparedPreviewPath();
    if (!path || this.preparedPreviewFamily() !== 'pdf') return null;
    try { return this.sanitizer.bypassSecurityTrustResourceUrl(convertFileSrc(path)); } catch { return null; }
  });
  protected readonly imagePreviewUrl = computed(() => {
    const path = this.preparedPreviewPath();
    if (!path || this.preparedPreviewFamily() !== 'image') return null;
    try { return convertFileSrc(path); } catch { return null; }
  });
  protected readonly resultPdfPreviewUrl = computed<SafeResourceUrl | null>(() => {
    const path = this.store.executionSummary()?.outputs?.[0];
    if (!path || this.pathFamily(path) !== 'pdf') return null;
    try { return this.sanitizer.bypassSecurityTrustResourceUrl(convertFileSrc(path)); } catch { return null; }
  });
  protected readonly resultImagePreviewUrl = computed(() => {
    const path = this.store.executionSummary()?.outputs?.[0];
    if (!path || this.pathFamily(path) !== 'image') return null;
    try { return convertFileSrc(path); } catch { return null; }
  });
  protected readonly fullscreenPdfUrl = computed<SafeResourceUrl | null>(() => {
    const path = this.fullscreenPreviewPath();
    if (!path || this.fullscreenPreviewFamily() !== 'pdf') return null;
    try { return this.sanitizer.bypassSecurityTrustResourceUrl(convertFileSrc(path)); } catch { return null; }
  });
  protected readonly fullscreenImageUrl = computed(() => {
    const path = this.fullscreenPreviewPath();
    if (!path || this.fullscreenPreviewFamily() !== 'image') return null;
    try { return convertFileSrc(path); } catch { return null; }
  });

  constructor() {
    const draft = this.memory.guidedFlowDraft();
    if (draft) {
      this.targetFormat.set(draft.targetFormat ?? 'pdf');
      this.quality.set(draft.quality ?? 'balanced');
      if (draft.destination === 'choose' && draft.customDirectory) {
        this.destination.set('choose');
        this.customDirectory.set(draft.customDirectory);
      }
      this.finalCompression.set(draft.finalCompression ?? 'balanced');
      this.improve.set(draft.improve ?? false);
      this.stripMetadata.set(draft.stripMetadata ?? false);
      this.targetSizeMb.set(draft.targetSizeMb ?? null);
      this.signatureText.set(draft.signatureText ?? '');
      this.collectionOrder.set(draft.collectionOrder ?? 'name');
      this.advancedOpen.set(draft.advancedOpen ?? false);
      if (draft.actionId && TASKS.some(task => task.id === draft.actionId)) this.selectedTask.set(draft.actionId as SimpleTask);
    }
    effect(() => {
      const preference = this.prefs.destination();
      if (this.destination() === 'choose' && this.customDirectory()) return;
      this.destination.set(preference === 'sameFolder' ? 'same' : preference === 'ask' ? 'choose' : 'subfolder');
      this.customDirectory.set(null);
    });
    effect(() => this.memory.saveGuidedFlowDraft({
      actionId: this.selectedTask(), targetFormat: this.targetFormat(), quality: this.quality(), destination: this.destination(), customDirectory: this.customDirectory(), finalCompression: this.finalCompression(), improve: this.improve(), stripMetadata: this.stripMetadata(), targetSizeMb: this.targetSizeMb(), signatureText: this.signatureText(), collectionOrder: this.collectionOrder(), advancedOpen: this.advancedOpen(),
    }));
    effect(() => {
      const asset = this.firstAsset();
      const request = ++this.previewRequest;
      this.preparedPreviewPath.set(null);
      this.preparedPreviewFamily.set('unknown');
      this.previewError.set(null);
      if (asset?.kind === 'file') void this.preparePreview(asset, request);
    });
    if (this.store.activeActionId()) this.selectedTask.set(null);
    if (!this.store.hasWorkspace()) void this.store.restoreRememberedWorkspace();
  }

  protected openSourcePreview(): void {
    const asset = this.firstAsset();
    if (asset) this.openAssetPreview(asset);
  }

  protected async openAssetPreview(asset: Asset): Promise<void> {
    if (asset.kind === 'archive') {
      void this.openArchiveBrowser(asset);
      return;
    }
    if (asset.kind !== 'file') return;
    let path = asset.data.path;
    let family = this.assetFamily(asset);
    try {
      const preview = await this.store.prepareAssetPreview(asset.data.id);
      path = preview.path;
      family = preview.family;
    } catch (error) {
      this.previewError.set(this.readableError(error));
    }
    this.fullscreenPreviewPath.set(path);
    this.fullscreenPreviewTitle.set(asset.data.name);
    this.fullscreenPreviewFamily.set(family);
  }

  protected openResultPreview(): void {
    const path = this.store.executionSummary()?.outputs?.[0];
    if (!path) return;
    this.fullscreenPreviewPath.set(path);
    this.fullscreenPreviewTitle.set(this.fileName(path));
    this.fullscreenPreviewFamily.set(this.pathFamily(path));
  }

  protected closeFullscreenPreview(): void {
    this.fullscreenPreviewPath.set(null);
    this.fullscreenPreviewFamily.set('unknown');
  }

  protected async openArchiveBrowser(asset: Asset | null = null): Promise<void> {
    const firstAsset = this.firstAsset();
    const archive = asset?.kind === 'archive'
      ? asset
      : firstAsset?.kind === 'archive' ? firstAsset : null;
    if (!archive || archive.kind !== 'archive') return;
    this.archiveBrowserTitle.set(archive.data.name);
    this.archivePage.set(0);
    this.archiveBrowserOpen.set(true);
    await this.store.inspectArchive(0, this.archivePageSize, archive.data.id);
  }

  protected closeArchiveBrowser(): void {
    this.archiveBrowserOpen.set(false);
    this.archiveEntryPreviewLoading.set(false);
    this.archivePreviewRequest += 1;
  }

  protected async archiveNext(): Promise<void> {
    const next = this.archivePage() + 1;
    this.archivePage.set(next);
    await this.store.inspectArchive(next * this.archivePageSize, this.archivePageSize);
  }

  protected async archivePrevious(): Promise<void> {
    const previous = Math.max(0, this.archivePage() - 1);
    this.archivePage.set(previous);
    await this.store.inspectArchive(previous * this.archivePageSize, this.archivePageSize);
  }

  protected async previewArchiveEntry(path: string): Promise<void> {
    if (this.archiveEntryPreviewLoading()) return;
    const request = ++this.archivePreviewRequest;
    this.archiveEntryPreviewName.set(this.archiveEntryName(path));
    this.archiveEntryPreviewLoading.set(true);
    try {
      const preview = await this.store.previewArchiveEntry(path);
      if (!preview || request !== this.archivePreviewRequest) return;
      this.fullscreenPreviewPath.set(preview.path);
      this.fullscreenPreviewTitle.set(this.archiveEntryName(path));
      this.fullscreenPreviewFamily.set(preview.family);
    } finally {
      if (request === this.archivePreviewRequest) this.archiveEntryPreviewLoading.set(false);
    }
  }

  protected archiveEntryName(path: string): string { return path.split(/[\\/]/).pop() || path; }
  protected familyMark(family: FormatFamily): string { return family === 'pdf' ? 'PDF' : family === 'image' ? 'IMG' : family === 'document' ? 'DOC' : family === 'spreadsheet' ? 'XLS' : family === 'presentation' ? 'PPT' : family === 'archive' ? 'ZIP' : family === 'video' ? 'VID' : family === 'audio' ? 'AUD' : 'TXT'; }
  protected fullscreenPreviewMark(): string { return this.familyMark(this.fullscreenPreviewFamily()); }
  private pathFamily(path: string): FormatFamily {
    const extension = (path.split('.').pop() || '').toLowerCase();
    if (extension === 'pdf') return 'pdf';
    if (['jpg','jpeg','png','webp','gif','bmp','tif','tiff','heic','heif','avif'].includes(extension)) return 'image';
    return 'unknown';
  }

  protected stepLabel(step: number): string { return ['','Accueil','Action','Configurer','Traitement','Résultat'][step] ?? ''; }
  protected selectionLabel(): string { return this.selectionCount() === 1 ? 'ce fichier' : `ces ${this.selectionCount()} fichiers`; }
  protected visibleSourceAssets(): Asset[] { return this.store.assets().filter(a => a.kind === 'file' || a.kind === 'archive').slice(0, 3); }
  protected sourceOverflow(): number { return Math.max(0, this.selectionCount() - this.visibleSourceAssets().length); }
  protected chooseTask(task: SimpleTask): void {
    this.store.resetExecutionResult();
    this.store.closeAction();
    this.selectedTask.set(task);
    this.pdfPassword.set('');
    this.signatureText.set(this.signatureText());
    const options = this.computeTargetOptions(task);
    if (!options.some(option => option.value === this.targetFormat())) this.targetFormat.set(options[0]?.value ?? 'pdf');
  }
  protected returnToActions(): void { this.store.resetExecutionResult(); this.store.closeAction(); this.selectedTask.set(null); this.pdfPassword.set(''); }
  protected async backHome(): Promise<void> { await this.router.navigate(['/']); }
  protected async chooseDestination(): Promise<void> { const paths = await this.store.pickDirectories(); if (paths.length) { this.customDirectory.set(paths[0]); this.destination.set('choose'); } }
  protected setTarget(value: string): void { this.targetFormat.set(value); }
  protected setTargetSize(value: string): void { const n = Number(value); this.targetSizeMb.set(Number.isFinite(n) && n > 0 ? n : null); }

  protected configureTitle(): string {
    if (this.store.activeActionId()) return this.capabilities.action(this.store.activeActionId())?.title ?? 'Configurer l’action';
    const task = TASKS.find(item => item.id === this.selectedTask());
    return task?.title ?? 'Configurer';
  }
  protected configureSubtitle(): string {
    if (this.selectedTask() === 'convert') return 'Choisissez seulement le résultat souhaité. FileFlow calcule les étapes intermédiaires.';
    if (this.selectedTask() === 'organize') return this.isSinglePdf() ? 'FileFlow peut diviser ce PDF proprement.' : 'Les fichiers seront normalisés puis regroupés dans un seul PDF.';
    if (this.selectedTask() === 'protect') return 'FileFlow crée une nouvelle copie protégée et conserve l’original.';
    return this.activeAction()?.description ?? 'Les réglages utiles seulement.';
  }
  protected destinationLabel(): string {
    if (this.destination() === 'choose') return this.customDirectory() ? `Dossier personnalisé · ${this.folderName(this.customDirectory()!)}` : 'Choisir un dossier';
    if (this.destination() === 'same') return 'À côté de l’original';
    const guided = this.prefs.beginnerMode() ? this.auth.onboarding()?.storageDirectory?.trim() : '';
    if (guided) return `Dossier FileFlow · ${this.folderName(guided)}`;
    return 'Sous-dossier FileFlow';
  }
  protected supportsQuality(): boolean { return ['convert','compress','organize'].includes(this.selectedTask() ?? '') || this.activeAction()?.category === 'optimize'; }
  protected canRun(): boolean {
    const action = this.activeAction();
    if (this.selectedTask() === 'rename') return true;
    if (!action || !this.capabilities.isActionExecutable(action)) return false;
    if (this.selectedTask() === 'protect' && !this.pdfPassword().trim()) return false;
    if (this.destination() === 'choose' && !this.customDirectory()) return false;
    return true;
  }
  protected launchLabel(): string { if (this.selectedTask() === 'rename') return 'Ouvrir le renommage'; if (this.selectedTask() === 'organize' && this.isSinglePdf()) return 'Diviser le PDF'; if (this.willProducePdf()) return 'Créer le PDF'; return `Lancer ${this.configureTitle().toLowerCase()}`; }

  protected async runCurrent(): Promise<void> {
    if (this.selectedTask() === 'rename') { await this.router.navigate(['/organize']); return; }
    const action = this.activeAction();
    const workspaceId = this.store.workspace()?.id;
    if (!action || !workspaceId || !this.capabilities.isActionExecutable(action)) return;
    if (this.selectedTask() === 'protect' && !this.pdfPassword().trim()) return;
    if (this.destination() === 'choose' && !this.customDirectory()) {
      await this.chooseDestination();
      if (!this.customDirectory()) return;
    }

    const resolvedDestination = resolveWorkspaceDestination(
      this.destination(),
      this.customDirectory(),
      this.prefs.beginnerMode() ? this.auth.onboarding()?.storageDirectory ?? null : null,
    );
    const parameters: Record<string, string | number | boolean | null> = {
      finalCompression: this.finalCompression(), improve: this.improve(), stripMetadata: this.stripMetadata(), targetSizeMb: this.targetSizeMb(), signatureText: this.signatureText().trim() || null, collectionOrder: this.collectionOrder(),
    };
    if (this.selectedTask() === 'protect') parameters['password'] = this.pdfPassword();

    await this.store.executeAction({
      workspaceId,
      actionId: action.id,
      selectedAssetIds: [...this.store.selectedIds()],
      targetFormat: this.actionTargetFormat(action),
      quality: this.supportsQuality() ? this.quality() : null,
      parameters,
      outputPolicy: { ...resolvedDestination, subfolderName: 'FileFlow', preserveTree: this.prefs.preserveTree(), conflict: 'increment', naming: 'original', overwriteOriginal: false },
    });
    this.pdfPassword.set('');
    this.showPdfPassword.set(false);
  }

  protected restart(): void { this.store.resetExecutionResult(); this.store.closeAction(); this.selectedTask.set(null); this.pdfPassword.set(''); this.memory.clearGuidedFlowDraft(); }
  protected processingLabel(): string { return this.activeAction()?.title ?? 'FileFlow prépare votre résultat'; }
  protected phaseRank(): number {
    if (!this.store.executionPhaseActive()) return this.store.executionProgress() > 0 ? 1 : 0;
    switch (this.store.executionPhase()) {
      case 'conversion': return 1;
      case 'assemblage': return 2;
      case 'finalisation': return 3;
      case 'validation': return 4;
      default: return 0;
    }
  }
  protected phaseDetail(): string {
    if (!this.store.executionPhaseActive()) return `${this.store.executionCompleted()} / ${Math.max(1,this.store.executionTotal() || this.selectionCount())} fichier(s)`;
    const completed = this.store.executionPhaseCompleted();
    const total = this.store.executionPhaseTotal();
    return this.store.executionPhase() === 'conversion' ? `${completed} / ${Math.max(1,total)} fichier(s)` : 'FileFlow prépare le traitement';
  }
  protected previewTitle(): string { return this.primaryFileName(); }
  protected primaryFileName(): string { return this.firstAsset()?.data.name ?? 'Votre sélection'; }
  protected primaryFileMark(): string { const a = this.firstAsset(); return a ? this.assetMark(a) : 'FF'; }

  protected smartSuggestedTask(): SimpleTask { return this.selectionCount() > 1 || this.store.counts().archives > 0 ? 'organize' : this.primaryFamily() === 'pdf' ? 'compress' : 'convert'; }
  protected smartSuggestionTitle(): string { const task = this.smartSuggestedTask(); return task === 'organize' ? 'Créer un seul PDF à partir de toute cette sélection' : task === 'compress' ? 'Ce PDF peut être préparé pour le partage' : 'Créer un PDF compatible en un clic'; }
  protected smartSuggestionText(): string { return this.smartSuggestedTask() === 'organize' ? 'Dossier ou ZIP : FileFlow convertit les éléments, les fusionne puis nettoie les intermédiaires.' : 'FileFlow choisit automatiquement la route la plus fidèle disponible sur cette machine.'; }

  protected routeSummary(): string {
    if (this.willProducePdf()) return 'Route PDF intelligente activée';
    return 'Conversion directe privilégiée';
  }
  protected routeDetail(): string {
    if (this.willProducePdf()) return 'Si aucune conversion directe fiable n’existe, FileFlow utilise un intermédiaire non destructif (PNG/TIFF/DOCX), valide le PDF final puis supprime le workspace temporaire.';
    return 'Les moteurs et formats intermédiaires restent invisibles dans le mode simple.';
  }

  private resolveActionId(): string | null {
    if (this.store.activeActionId()) return this.store.activeActionId();
    const task = this.selectedTask();
    const family = this.primaryFamily();
    const target = this.targetFormat();
    if (!task) return null;
    if (task === 'convert') {
      if (target === 'pdf') return 'smart-to-pdf';
      if (family === 'image') return 'image-convert';
      if (['document','spreadsheet','presentation'].includes(family)) return 'office-convert';
      if (family === 'audio') return 'audio-convert';
      if (family === 'video') return target === 'gif' ? 'video-to-gif' : 'video-convert';
      if (family === 'text') return target === 'pdf' ? 'text-to-pdf' : 'text-convert';
      if (family === 'ebook') return target === 'pdf' ? 'smart-to-pdf' : 'ebook-convert';
      if (family === 'pdf') return target === 'txt' ? 'pdf-extract-text' : target === 'pdf' ? 'smart-to-pdf' : 'pdf-to-images';
      return 'smart-to-pdf';
    }
    if (task === 'compress') {
      if (family === 'pdf') return 'pdf-compress';
      if (family === 'image') return 'image-optimize';
      if (family === 'audio' || family === 'video') return 'media-compress';
      if (family === 'archive') return 'archive-package';
      return 'smart-to-pdf';
    }
    if (task === 'extract') {
      if (family === 'pdf') return 'pdf-extract-text';
      if (family === 'image') return 'ocr-image';
      if (family === 'archive') return 'archive-extract';
      if (family === 'video') return 'extract-audio';
      return 'smart-to-pdf';
    }
    if (task === 'organize') return this.isSinglePdf() ? 'pdf-split' : 'collection-to-pdf';
    if (task === 'protect') return family === 'pdf' ? 'pdf-protect' : 'smart-to-pdf';
    return null;
  }

  private actionTargetFormat(action: ActionDescriptor): string | null {
    if (['smart-to-pdf','collection-to-pdf','pdf-compress','pdf-protect','pdf-split','pdf-extract-text','ocr-image','archive-extract'].includes(action.id)) return null;
    if (action.id === 'archive-package') return 'zip';
    if (action.id === 'pdf-to-images') return this.targetFormat() === 'txt' ? null : this.targetFormat();
    return this.targetFormat();
  }

  private computeTargetOptions(task = this.selectedTask()): { value: string; label: string }[] {
    if (task === 'organize') return [{ value: 'pdf', label: 'PDF unique' }];
    if (task === 'protect') return [{ value: 'pdf', label: 'PDF protégé' }];
    if (task !== 'convert') return [{ value: this.targetFormat() || 'pdf', label: this.targetFormat().toUpperCase() || 'PDF' }];
    if (this.selectionCount() > 1 || this.store.counts().archives > 0) return [{ value: 'pdf', label: 'PDF' }];
    switch (this.primaryFamily()) {
      case 'image': return options(['pdf','jpg','png','webp','tiff']);
      case 'pdf': return [{value:'pdf',label:'PDF (finaliser)'},{value:'png',label:'Images PNG'},{value:'txt',label:'Texte'}];
      case 'document': return options(['pdf','docx','odt','rtf']);
      case 'spreadsheet': return options(['pdf','xlsx','ods','csv']);
      case 'presentation': return options(['pdf','pptx','odp']);
      case 'text': return options(['pdf','docx','html','md','txt']);
      case 'ebook': return options(['pdf','docx','html','txt','epub']);
      case 'audio': return options(['mp3','m4a','wav','flac']);
      case 'video': return options(['mp4','webm','mkv','mov','gif']);
      case 'archive': return [{value:'pdf',label:'PDF unique'},{value:'zip',label:'ZIP'}];
      default: return [{ value: 'pdf', label: 'PDF' }];
    }
  }

  protected assetFamily(asset: Asset): FormatFamily { return asset.kind === 'file' || asset.kind === 'archive' ? asset.data.format.family : 'unknown'; }
  protected assetFormat(asset: Asset): string { return asset.kind === 'file' || asset.kind === 'archive' ? (asset.data.format.extension || asset.data.format.id).toUpperCase() : asset.kind.toUpperCase(); }
  protected assetSize(asset: Asset): string { return asset.kind === 'file' || asset.kind === 'archive' ? this.formatBytes(asset.data.sizeBytes) : '—'; }
  protected assetMark(asset: Asset): string { const family = this.assetFamily(asset); return family === 'pdf' ? 'PDF' : family === 'image' ? 'IMG' : family === 'archive' ? 'ZIP' : family === 'video' ? 'VID' : family === 'audio' ? 'AUD' : family === 'document' ? 'DOC' : family === 'spreadsheet' ? 'XLS' : family === 'presentation' ? 'PPT' : 'TXT'; }
  protected formatBytes(bytes: number): string { if (!bytes) return '0 o'; const units=['o','Ko','Mo','Go','To']; const i=Math.min(units.length-1,Math.floor(Math.log(bytes)/Math.log(1024))); return `${(bytes/1024**i).toFixed(i===0?0:1)} ${units[i]}`; }
  protected fileName(path: string): string { return path.split(/[\\/]/).pop() || path; }
  protected extensionLabel(path: string): string { const ext=this.fileName(path).split('.').pop(); return ext ? ext.toUpperCase() : 'Fichier'; }
  protected resultMark(path: string): string { const ext=this.extensionLabel(path); return ext === 'PDF' ? 'PDF' : ext.slice(0,3); }

  protected retryPreview(): void {
    const asset = this.firstAsset();
    if (asset?.kind === 'file') void this.preparePreview(asset, ++this.previewRequest);
  }

  private async preparePreview(asset: Asset & { kind: 'file' }, request: number): Promise<void> {
    this.previewLoading.set(true);
    this.previewError.set(null);
    try {
      const preview = await this.store.prepareAssetPreview(asset.data.id);
      if (request !== this.previewRequest) return;
      this.preparedPreviewPath.set(preview.path);
      this.preparedPreviewFamily.set(preview.family);
    } catch (error) {
      if (request === this.previewRequest) this.previewError.set(this.readableError(error));
    } finally {
      if (request === this.previewRequest) this.previewLoading.set(false);
    }
  }

  private folderName(path: string): string { return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || 'FileFlow'; }
  private readableError(error: unknown): string { return error instanceof Error ? error.message : String(error || 'Aperçu indisponible.'); }
}

function options(values: string[]): { value: string; label: string }[] { return values.map(value => ({ value, label: value === 'jpg' ? 'JPEG / JPG' : value === 'tiff' ? 'TIFF' : value.toUpperCase() })); }
