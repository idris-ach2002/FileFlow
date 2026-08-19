import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'ff-home-page',
  template: `
    <header class="page-header">
      <p class="eyebrow">FILEFLOW</p>
      <h1>Que souhaitez-vous faire ?</h1>
      <p>Déposez un fichier, plusieurs fichiers, un dossier ou une archive. Le moteur proposera les actions pertinentes.</p>
    </header>

    <section class="drop-zone" aria-label="Zone de dépôt de fichiers">
      <div class="drop-icon">+</div>
      <strong>Déposez fichiers ou dossiers ici</strong>
      <span>L’analyse et les conversions resteront locales.</span>
      <div class="actions">
        <button type="button">Choisir des fichiers</button>
        <button type="button" class="secondary">Choisir un dossier</button>
      </div>
    </section>

    <section class="quick-actions">
      <article><strong>Créer un PDF</strong><span>Images, documents et scans.</span></article>
      <article><strong>Réduire la taille</strong><span>PDF, images et médias.</span></article>
      <article><strong>Fusionner</strong><span>Réunir plusieurs documents.</span></article>
      <article><strong>Extraire</strong><span>Texte, pages ou archives.</span></article>
    </section>
  `,
  styles: [`
    :host { display: block; max-width: 1080px; margin: 0 auto; }
    .page-header { max-width: 720px; }
    .eyebrow { margin: 0 0 10px; color: var(--accent); font-size: 12px; font-weight: 800; letter-spacing: .14em; }
    h1 { margin: 0; font-size: clamp(34px, 5vw, 54px); letter-spacing: -.04em; }
    .page-header > p:last-child { color: var(--text-muted); line-height: 1.7; font-size: 16px; }
    .drop-zone { margin-top: 34px; min-height: 280px; display: grid; place-items: center; align-content: center; gap: 10px; padding: 34px; border: 1.5px dashed #bdc6d6; border-radius: 22px; background: var(--surface-1); text-align: center; }
    .drop-icon { width: 52px; height: 52px; display: grid; place-items: center; margin-bottom: 4px; border-radius: 16px; background: var(--accent-soft); color: var(--accent); font-size: 30px; }
    .drop-zone span { color: var(--text-muted); }
    .actions { display: flex; gap: 10px; margin-top: 14px; }
    button { border: 0; border-radius: 10px; padding: 11px 16px; background: var(--accent); color: white; font-weight: 700; }
    button.secondary { background: var(--surface-2); color: var(--text); }
    .quick-actions { margin-top: 22px; display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; }
    article { min-height: 112px; display: flex; flex-direction: column; gap: 7px; padding: 18px; border: 1px solid var(--border); border-radius: 16px; background: var(--surface-1); }
    article span { color: var(--text-muted); line-height: 1.4; font-size: 13px; }
    @media (max-width: 1000px) { .quick-actions { grid-template-columns: repeat(2, 1fr); } }
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {}
