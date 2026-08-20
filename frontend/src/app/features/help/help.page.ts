import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { ActionDescriptor } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from '../workspace/data-access/workspace.store';

interface HelpGuide {
  id: string;
  title: string;
  summary: string;
  keywords: string;
  steps: string[];
  actionId?: string;
}

const ESSENTIAL_GUIDES: HelpGuide[] = [
  { id:'start', title:'Je ne sais pas par où commencer', summary:'Le chemin le plus simple pour une première utilisation.', keywords:'début commencer premier aide', steps:['Choisissez ce que vous voulez obtenir sur l’accueil.','Ajoutez un fichier, plusieurs fichiers ou un dossier.','FileFlow vous montre uniquement les actions compatibles.','Vérifiez la destination puis lancez le traitement.'] },
  { id:'pdf-smaller', title:'Réduire la taille d’un PDF', summary:'Créer une copie plus légère sans modifier l’original.', keywords:'pdf réduire compresser taille email léger', actionId:'pdf-compress', steps:['Choisissez « PDF & documents » puis « Compresser un PDF ».','Ajoutez votre PDF.','Choisissez Léger, Équilibré ou Haute qualité.','Lancez : le fichier original reste intact.'] },
  { id:'merge-pdf', title:'Réunir plusieurs PDF', summary:'Fusionner des documents dans l’ordre voulu.', keywords:'fusion pdf réunir assembler', actionId:'pdf-merge', steps:['Choisissez « Fusionner des PDF ».','Ajoutez au moins deux PDF.','Vérifiez leur ordre dans la liste.','Lancez la fusion et ouvrez le nouveau PDF.'] },
  { id:'iphone', title:'Convertir des photos iPhone HEIC', summary:'Créer des JPG ou WebP faciles à partager.', keywords:'iphone apple heic heif photo jpg jpeg webp', actionId:'image-batch-convert', steps:['Choisissez « Photos & images ».','Ajoutez vos HEIC, même par centaines.','Choisissez JPG pour la compatibilité ou WebP pour le web.','FileFlow crée les nouvelles images en lot.'] },
  { id:'scan', title:'Rendre un scan PDF recherchable', summary:'Utiliser l’OCR pour sélectionner et rechercher le texte.', keywords:'scan ocr texte searchable pdf', actionId:'pdf-ocr', steps:['Choisissez « Lire un document scanné ».','Ajoutez le PDF du scanner.','Lancez l’OCR.','Ouvrez le résultat puis essayez de sélectionner le texte.'] },
  { id:'archive', title:'Ouvrir un ZIP, 7Z, RAR ou TAR', summary:'Inspecter le contenu avant une extraction protégée.', keywords:'zip 7z rar tar archive décompresser extraire', actionId:'archive-extract', steps:['Ajoutez l’archive.','FileFlow affiche les types de fichiers qu’elle contient.','Vérifiez le dossier de destination.','Extrayez : les chemins dangereux et archives suspectes sont refusés.'] },
  { id:'zstd', title:'Compresser très vite avec Zstandard', summary:'Compression sans perte pensée pour la vitesse.', keywords:'zstd zstandard rapide compression sans perte', actionId:'zstd-compress', steps:['Choisissez « Compresser ».','Ajoutez un ou plusieurs fichiers.','Sélectionnez Rapide, Équilibré ou Maximum.','FileFlow utilise un nombre de threads borné pour garder l’ordinateur réactif.'] },
  { id:'video', title:'Rendre une vidéo compatible partout', summary:'Créer un MP4 lisible sur téléphone, TV et navigateur.', keywords:'video mp4 mov mkv avi compatible ffmpeg', actionId:'media-compatible', steps:['Choisissez « Audio & vidéo ».','Ajoutez votre vidéo.','Choisissez « Rendre compatible ».','FileFlow crée un MP4 optimisé sans toucher à la source.'] },
  { id:'ebook', title:'Convertir un livre EPUB ou FB2', summary:'Créer une copie HTML, Markdown, DOCX, TXT ou EPUB.', keywords:'ebook livre epub fb2 convertir docx html markdown txt', actionId:'ebook-convert', steps:['Choisissez « PDF & documents ».','Ajoutez un livre EPUB ou FB2.','Choisissez le format de sortie souhaité.','FileFlow lance Pandoc en mode isolé et conserve le livre original.'] },
  { id:'privacy', title:'Retirer GPS et métadonnées avant partage', summary:'Créer une copie nettoyée pour protéger votre vie privée.', keywords:'gps exif privé metadata confidentialité', actionId:'strip-metadata', steps:['Choisissez « Confidentialité ».','Ajoutez les photos ou médias à partager.','Vérifiez l’avertissement : cette action modifie la copie produite.','Lancez puis partagez uniquement le résultat nettoyé.'] },
  { id:'duplicates', title:'Trouver les vrais doublons', summary:'Comparer le contenu, pas seulement le nom.', keywords:'doublon duplicate espace disque nettoyage', actionId:'duplicate-scan', steps:['Ajoutez le dossier à analyser.','Ouvrez « Ranger & nettoyer ».','Lancez la recherche de doublons.','FileFlow confirme avec des empreintes complètes avant d’indiquer des fichiers identiques.'] },
  { id:'destination', title:'Changer l’endroit où arrivent les résultats', summary:'Même dossier, sous-dossier ou dossier FileFlow.', keywords:'destination stockage résultat dossier paramètres', steps:['Ouvrez Paramètres.','Choisissez « Fichiers & stockage ».','Sélectionnez la destination par défaut.','Vous pourrez toujours la changer au cas par cas avant une action.'] },
  { id:'engines', title:'Une fonction indique « moteur manquant »', summary:'Comprendre et réparer une capacité locale indisponible.', keywords:'moteur manquant installer ffmpeg vips qpdf', steps:['Ouvrez Paramètres puis « Moteurs locaux ».','Repérez le moteur marqué indisponible.','Lancez scripts/setup.sh dans le projet pour installer les outils optionnels.','Revenez dans FileFlow et actualisez le diagnostic.'] },
];

@Component({
  selector: 'ff-help-page',
  template: `
    <div class="help-shell">
      <header class="help-hero">
        <div><p class="ff-kicker">AIDE & GUIDES</p><h1>Qu’est-ce que vous voulez faire ?</h1><p>Écrivez avec vos mots. Pas besoin de connaître le nom d’un format ou d’un outil.</p></div>
        <div class="search-box"><span>⌕</span><input #search type="search" autofocus placeholder="Ex. réduire un PDF, photos iPhone, ouvrir un ZIP…" [value]="query()" (input)="query.set(search.value)" /><kbd>⌘K</kbd></div>
      </header>

      <section class="quick-questions">
        @for (question of quickQuestions; track question) { <button type="button" (click)="query.set(question)">{{ question }}</button> }
      </section>

      <div class="help-layout">
        <main class="guide-list">
          <div class="section-title"><h2>{{ query() ? 'Résultats' : 'Guides essentiels' }}</h2><span>{{ results().length }} guide{{ results().length > 1 ? 's' : '' }}</span></div>
          @for (guide of results(); track guide.id) {
            <article class="guide-card ff-card" [class.open]="openGuide() === guide.id">
              <button class="guide-head" type="button" (click)="toggle(guide.id)">
                <span class="guide-icon">?</span><span><strong>{{ guide.title }}</strong><small>{{ guide.summary }}</small></span><b>{{ openGuide() === guide.id ? '−' : '+' }}</b>
              </button>
              @if (openGuide() === guide.id) {
                <div class="guide-body">
                  <ol>@for (step of guide.steps; track step) { <li><span>{{ $index + 1 }}</span><p>{{ step }}</p></li> }</ol>
                  @if (guide.actionId) { <button class="ff-button" type="button" (click)="launch(guide.actionId)">Faire cette action maintenant</button> }
                </div>
              }
            </article>
          } @empty { <div class="empty"><strong>Aucun guide essentiel exact.</strong><span>Regardez aussi les actions correspondantes à droite.</span></div> }
        </main>

        <aside class="action-index ff-card">
          <p class="ff-kicker">TOUTES LES ACTIONS</p><h2>Mode d’emploi intégré</h2><p>Chaque capacité de FileFlow a son mini-guide, généré depuis le catalogue réel.</p>
          @if (selectedAction(); as action) {
            <div class="action-howto">
              <button class="close" type="button" (click)="selectedAction.set(null)">×</button>
              <strong>{{ action.title }}</strong><span>{{ action.description }}</span>
              <ol><li><b>1</b> Ajoutez {{ action.batchable ? 'le ou les fichiers concernés' : 'le fichier concerné' }}.</li><li><b>2</b> FileFlow vérifie que leur format est compatible.</li><li><b>3</b> Choisissez la qualité et la destination si elles sont proposées.</li><li><b>4</b> Lancez l’action puis ouvrez le résultat.</li></ol>
              <button class="ff-button" type="button" (click)="launch(action.id)">Commencer</button>
            </div>
          }
          <div class="action-links">
            @for (action of filteredActions(); track action.id) {
              <button type="button" (click)="selectedAction.set(action)"><span>{{ action.title }}</span><small>{{ action.description }}</small></button>
            }
          </div>
        </aside>
      </div>
    </div>
  `,
  styles: [`
    :host{display:block}.help-shell{max-width:1180px;margin:0 auto}.help-hero{display:grid;grid-template-columns:minmax(0,1fr) minmax(320px,480px);gap:30px;align-items:end}.help-hero h1{max-width:650px;margin:0;font-size:clamp(38px,5vw,58px);line-height:1.02;letter-spacing:-.055em}.help-hero p:last-child{margin:12px 0 0;color:var(--text-muted);line-height:1.6}.search-box{height:54px;display:grid;grid-template-columns:24px minmax(0,1fr) auto;align-items:center;gap:8px;padding:0 13px;border:1px solid var(--border-strong);border-radius:15px;background:var(--surface-1);box-shadow:var(--shadow-sm)}.search-box span{color:var(--text-faint);font-size:20px}.search-box input{width:100%;border:0;outline:0;background:transparent;color:var(--text);font-size:13px}.quick-questions{display:flex;flex-wrap:wrap;gap:7px;margin:24px 0 34px}.quick-questions button{min-height:34px;padding:0 12px;border:1px solid var(--border);border-radius:999px;background:var(--surface-1);color:var(--text-muted);font-size:11px;font-weight:700}.quick-questions button:hover{border-color:var(--accent);color:var(--accent)}.help-layout{display:grid;grid-template-columns:minmax(0,1fr) 340px;gap:16px;align-items:start}.section-title{display:flex;justify-content:space-between;align-items:center;margin-bottom:12px}.section-title h2{margin:0;font-size:18px}.section-title span{color:var(--text-faint);font-size:11px}.guide-list{display:grid;gap:9px}.guide-card{overflow:hidden}.guide-head{width:100%;min-height:78px;display:grid;grid-template-columns:40px minmax(0,1fr) auto;align-items:center;gap:12px;padding:13px 16px;border:0;background:transparent;color:var(--text);text-align:left}.guide-head:hover{background:var(--surface-2)}.guide-icon{width:36px;height:36px;display:grid;place-items:center;border-radius:11px;background:var(--accent-soft);color:var(--accent);font-weight:900}.guide-head strong,.guide-head small{display:block}.guide-head strong{font-size:13px}.guide-head small{margin-top:4px;color:var(--text-muted);font-size:11px;line-height:1.4}.guide-head b{color:var(--text-faint);font-size:20px}.guide-body{padding:0 16px 18px 68px;border-top:1px solid var(--border)}ol{list-style:none;margin:14px 0;padding:0;display:grid;gap:9px}li{display:grid;grid-template-columns:25px minmax(0,1fr);gap:8px;align-items:start}li>span,li>b{width:23px;height:23px;display:grid;place-items:center;border-radius:7px;background:var(--surface-2);color:var(--text-muted);font-size:10px;font-weight:850}li p{margin:3px 0 0;color:var(--text-muted);font-size:12px;line-height:1.5}.action-index{position:sticky;top:24px;padding:17px}.action-index h2{margin:0;font-size:20px}.action-index>p:not(.ff-kicker){margin:7px 0 14px;color:var(--text-muted);font-size:12px;line-height:1.55}.action-howto{position:relative;margin:0 0 12px;padding:13px;border:1px solid color-mix(in srgb,var(--accent) 22%,var(--border));border-radius:12px;background:var(--accent-soft)}.action-howto>strong,.action-howto>span{display:block}.action-howto>strong{padding-right:25px;font-size:12px}.action-howto>span{margin-top:4px;color:var(--text-muted);font-size:11px;line-height:1.5}.action-howto ol{font-size:11px;color:var(--text-muted)}.action-howto li{grid-template-columns:23px 1fr}.close{position:absolute;right:7px;top:6px;border:0;background:transparent;color:var(--text-muted);font-size:18px}.action-links{max-height:550px;overflow:auto;display:grid;gap:3px}.action-links button{padding:10px;border:0;border-radius:9px;background:transparent;color:var(--text);text-align:left}.action-links button:hover{background:var(--surface-2)}.action-links span,.action-links small{display:block}.action-links span{font-size:12px;font-weight:800}.action-links small{margin-top:3px;color:var(--text-faint);font-size:10.5px;line-height:1.45}.empty{padding:40px;border:1px dashed var(--border);border-radius:15px;text-align:center}.empty strong,.empty span{display:block}.empty span{margin-top:6px;color:var(--text-muted);font-size:11px}@media(max-width:940px){.help-hero,.help-layout{grid-template-columns:1fr}.action-index{position:static}}@media(max-width:620px){.guide-body{padding-left:16px}.help-hero h1{font-size:40px}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HelpPage {
  private readonly capabilities = inject(CapabilityStore);
  private readonly workspace = inject(WorkspaceStore);
  private readonly router = inject(Router);
  protected readonly query = signal('');
  protected readonly openGuide = signal<string | null>(null);
  protected readonly selectedAction = signal<ActionDescriptor | null>(null);
  protected readonly quickQuestions = ['Réduire un PDF','Photos iPhone HEIC','Ouvrir un ZIP','Compresser très vite','Convertir un ebook','Trouver des doublons','Retirer le GPS'];

  protected readonly results = computed(() => {
    const query = normalize(this.query());
    if (!query) return ESSENTIAL_GUIDES;
    const parts = query.split(/\s+/).filter(Boolean);
    return ESSENTIAL_GUIDES.filter((guide) => {
      const corpus = normalize(`${guide.title} ${guide.summary} ${guide.keywords}`);
      return corpus.includes(query) || parts.every((part) => corpus.includes(part));
    });
  });

  protected readonly filteredActions = computed(() => {
    const query = normalize(this.query());
    const actions = this.capabilities.actions();
    if (!query) return actions.slice(0, 50);
    const parts = query.split(/\s+/).filter(Boolean);
    return actions.filter((action) => {
      const corpus = normalize(`${action.title} ${action.description} ${action.category}`);
      return corpus.includes(query) || parts.every((part) => corpus.includes(part));
    }).slice(0, 50);
  });

  protected toggle(id: string): void { this.openGuide.update((current) => current === id ? null : id); }
  protected launch(actionId: string): void {
    this.workspace.setPendingAction(actionId);
    if (this.workspace.hasWorkspace()) {
      this.workspace.openAction(actionId);
      void this.router.navigate(['/workspace']);
    } else {
      void this.router.navigate(['/']);
    }
  }
}

function normalize(value: string): string {
  return value.toLocaleLowerCase('fr').normalize('NFD').replace(/[\u0300-\u036f]/g, '').trim();
}
