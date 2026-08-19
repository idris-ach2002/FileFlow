import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { Asset, FormatFamily } from '../../core/ipc/tauri.models';
import { WorkspaceStore } from './data-access/workspace.store';

@Component({
  selector: 'ff-workspace-page',
  template: `
    <header class="workspace-header">
      <div>
        <p class="eyebrow">ESPACE FICHIERS</p>
        <h1>Votre sélection</h1>
        <p>FileFlow conserve les fichiers originaux et prépare ici les actions possibles.</p>
      </div>
      <button type="button" class="new-button" [disabled]="store.busy()" (click)="newSelection()">Nouvelle sélection</button>
    </header>

    @switch (store.phase()) {
      @case ('idle') {
        <section class="empty-state">
          <div class="empty-icon">＋</div>
          <h2>Aucun fichier pour le moment</h2>
          <p>Revenez à l’accueil pour choisir des fichiers, un dossier ou déposer directement des éléments dans la fenêtre.</p>
          <button type="button" (click)="newSelection()">Choisir des fichiers</button>
        </section>
      }

      @case ('error') {
        <section class="error-state">
          <strong>L’analyse n’a pas pu être terminée.</strong>
          <span>{{ store.error() }}</span>
          <button type="button" (click)="newSelection()">Recommencer</button>
        </section>
      }

      @default {
        <section class="summary-grid" aria-label="Résumé du workspace">
          <article><span>Éléments</span><strong>{{ store.counts().assets }}</strong></article>
          <article><span>Fichiers</span><strong>{{ store.counts().files }}</strong></article>
          <article><span>Dossiers</span><strong>{{ store.counts().directories }}</strong></article>
          <article><span>Archives</span><strong>{{ store.counts().archives }}</strong></article>
          <article><span>Taille</span><strong>{{ formatBytes(store.counts().totalBytes) }}</strong></article>
        </section>

        @if (store.busy()) {
          <section class="scan-status">
            <div>
              <span class="pulse"></span>
              <div><strong>Analyse en cours</strong><small>{{ store.stats().discovered }} éléments détectés</small></div>
            </div>
            <span>Les résultats apparaissent progressivement.</span>
          </section>
        }

        @if (store.workspace(); as workspace) {
          @if (workspace.families.length > 0) {
            <nav class="family-filters" aria-label="Filtrer les fichiers par type">
              <button type="button" [class.active]="store.familyFilter() === null" (click)="filterFamily(null)">Tous</button>
              @for (entry of workspace.families; track entry.family) {
                <button type="button" [class.active]="store.familyFilter() === entry.family" (click)="filterFamily(entry.family)">
                  {{ familyLabel(entry.family) }} <span>{{ entry.count }}</span>
                </button>
              }
            </nav>
          }
        }

        <section class="asset-panel">
          <div class="asset-panel-head">
            <div>
              <strong>{{ store.pageTotal() || store.assets().length }} éléments</strong>
              @if (store.warnings().length > 0) {
                <span>{{ store.warnings().length }} avertissement(s)</span>
              }
            </div>
            <span class="local-badge">100 % local</span>
          </div>

          <div class="asset-list">
            @for (asset of store.assets(); track asset.data.id) {
              <article class="asset-row">
                <div class="asset-icon" [attr.data-family]="assetFamily(asset)">{{ assetMark(asset) }}</div>
                <div class="asset-main">
                  <strong>{{ asset.data.name }}</strong>
                  <span>{{ asset.data.relativePath }}</span>
                </div>
                <div class="asset-format">
                  <strong>{{ assetFormat(asset) }}</strong>
                  <span>{{ familyLabel(assetFamily(asset)) }}</span>
                </div>
                <div class="asset-size">{{ assetSize(asset) }}</div>
              </article>
            } @empty {
              <div class="asset-empty">Aucun élément dans ce filtre.</div>
            }
          </div>

          @if (store.hasMore()) {
            <div class="load-more">
              <button type="button" (click)="loadMore()">Afficher plus</button>
            </div>
          }
        </section>
      }
    }
  `,
  styles: [`
    :host { display: block; max-width: 1180px; margin: 0 auto; }
    .workspace-header { display: flex; justify-content: space-between; gap: 24px; align-items: flex-start; }
    .eyebrow { margin: 0 0 8px; color: var(--accent); font-size: 12px; font-weight: 800; letter-spacing: .14em; }
    h1 { margin: 0; font-size: 42px; letter-spacing: -.04em; }
    .workspace-header p:last-child { margin: 10px 0 0; color: var(--text-muted); }
    button { border: 0; border-radius: 10px; padding: 10px 14px; font-weight: 700; }
    .new-button { background: var(--surface-1); color: var(--text); border: 1px solid var(--border); }
    button:disabled { opacity: .5; cursor: default; }
    .summary-grid { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 10px; margin-top: 28px; }
    .summary-grid article { display: grid; gap: 7px; padding: 16px 17px; border: 1px solid var(--border); border-radius: 14px; background: var(--surface-1); }
    .summary-grid span { color: var(--text-muted); font-size: 12px; }
    .summary-grid strong { font-size: 20px; letter-spacing: -.02em; }
    .scan-status { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-top: 14px; padding: 13px 16px; border: 1px solid color-mix(in srgb, var(--accent) 20%, var(--border)); border-radius: 13px; background: var(--accent-soft); color: var(--text-muted); font-size: 13px; }
    .scan-status > div { display: flex; align-items: center; gap: 10px; color: var(--text); }
    .scan-status strong, .scan-status small { display: block; }
    .scan-status small { margin-top: 2px; color: var(--text-muted); }
    .pulse { width: 9px; height: 9px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 5px color-mix(in srgb, var(--accent) 15%, transparent); }
    .family-filters { display: flex; flex-wrap: wrap; gap: 7px; margin: 20px 0 12px; }
    .family-filters button { padding: 8px 11px; background: var(--surface-1); color: var(--text-muted); border: 1px solid var(--border); font-size: 12px; }
    .family-filters button.active { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 35%, var(--border)); background: var(--accent-soft); }
    .family-filters button span { margin-left: 5px; opacity: .65; }
    .asset-panel { margin-top: 18px; overflow: hidden; border: 1px solid var(--border); border-radius: 16px; background: var(--surface-1); }
    .asset-panel-head { min-height: 58px; display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
    .asset-panel-head strong, .asset-panel-head span { display: block; }
    .asset-panel-head div > span { margin-top: 2px; color: var(--text-muted); font-size: 11px; }
    .local-badge { padding: 5px 8px; border-radius: 999px; background: #e9f8f1; color: #16835a; font-size: 11px; font-weight: 800; }
    .asset-list { display: grid; }
    .asset-row { display: grid; grid-template-columns: 38px minmax(0, 1fr) 130px 88px; align-items: center; gap: 12px; min-height: 65px; padding: 9px 16px; border-bottom: 1px solid var(--border); }
    .asset-row:last-child { border-bottom: 0; }
    .asset-icon { width: 36px; height: 36px; display: grid; place-items: center; border-radius: 10px; background: var(--surface-2); color: var(--text-muted); font-size: 10px; font-weight: 900; text-transform: uppercase; }
    .asset-icon[data-family='image'] { background: #eef5ff; color: #2868c7; }
    .asset-icon[data-family='pdf'] { background: #fff0ef; color: #bf443e; }
    .asset-icon[data-family='archive'] { background: #fff6df; color: #9a6d00; }
    .asset-icon[data-family='document'], .asset-icon[data-family='text'] { background: #eff3ff; color: #5267bc; }
    .asset-main, .asset-format { min-width: 0; }
    .asset-main strong, .asset-main span, .asset-format strong, .asset-format span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .asset-main strong { font-size: 13px; }
    .asset-main span, .asset-format span { margin-top: 3px; color: var(--text-muted); font-size: 11px; }
    .asset-format strong { font-size: 11px; text-transform: uppercase; }
    .asset-size { color: var(--text-muted); font-size: 12px; text-align: right; }
    .asset-empty { padding: 50px 20px; text-align: center; color: var(--text-muted); }
    .load-more { display: flex; justify-content: center; padding: 12px; border-top: 1px solid var(--border); }
    .load-more button, .empty-state button, .error-state button { background: var(--accent); color: white; }
    .empty-state, .error-state { max-width: 620px; margin: 70px auto 0; display: grid; justify-items: center; gap: 10px; padding: 42px; text-align: center; border: 1px solid var(--border); border-radius: 20px; background: var(--surface-1); }
    .empty-state h2 { margin: 4px 0 0; }
    .empty-state p, .error-state span { margin: 0 0 10px; color: var(--text-muted); line-height: 1.6; }
    .empty-icon { width: 48px; height: 48px; display: grid; place-items: center; border-radius: 14px; background: var(--accent-soft); color: var(--accent); font-size: 26px; }
    @media (max-width: 1000px) { .summary-grid { grid-template-columns: repeat(3, 1fr); } .asset-row { grid-template-columns: 38px minmax(0, 1fr) 80px; } .asset-format { display: none; } }
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WorkspacePage {
  protected readonly store = inject(WorkspaceStore);
  private readonly router = inject(Router);

  protected newSelection(): void {
    void this.router.navigate(['/']);
  }

  protected filterFamily(family: FormatFamily | null): void {
    void this.store.setFamilyFilter(family);
  }

  protected loadMore(): void {
    void this.store.loadMore();
  }

  protected familyLabel(family: FormatFamily): string {
    return FAMILY_LABELS[family];
  }

  protected assetFamily(asset: Asset): FormatFamily {
    if (asset.kind === 'file' || asset.kind === 'archive') {
      return asset.data.format.family;
    }
    return 'unknown';
  }

  protected assetFormat(asset: Asset): string {
    switch (asset.kind) {
      case 'directory': return 'Dossier';
      case 'symlink': return 'Lien';
      case 'archive':
      case 'file': return asset.data.format.id;
    }
  }

  protected assetMark(asset: Asset): string {
    switch (asset.kind) {
      case 'directory': return 'DIR';
      case 'symlink': return 'LNK';
      case 'archive': return 'ZIP';
      case 'file': return asset.data.format.extension ?? asset.data.format.id.slice(0, 4);
    }
  }

  protected assetSize(asset: Asset): string {
    if (asset.kind === 'file' || asset.kind === 'archive') {
      return this.formatBytes(asset.data.sizeBytes);
    }
    return '—';
  }

  protected formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} o`;
    const units = ['Ko', 'Mo', 'Go', 'To'];
    let value = bytes / 1024;
    let unit = units[0];
    for (let index = 1; index < units.length && value >= 1024; index += 1) {
      value /= 1024;
      unit = units[index];
    }
    return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
  }
}

const FAMILY_LABELS: Record<FormatFamily, string> = {
  image: 'Images',
  pdf: 'PDF',
  document: 'Documents',
  spreadsheet: 'Tableurs',
  presentation: 'Présentations',
  audio: 'Audio',
  video: 'Vidéos',
  archive: 'Archives',
  ebook: 'Livres',
  text: 'Texte',
  unknown: 'Autres',
};
