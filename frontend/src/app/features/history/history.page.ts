import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'ff-history-page',
  template: `
    <section class="placeholder">
      <p>FILEFLOW</p>
      <h1>Historique</h1>
      <span>Les opérations terminées, destinations et actions « refaire » seront disponibles ici.</span>
    </section>
  `,
  styles: [`
    :host { display: block; }
    .placeholder { max-width: 760px; padding-top: 40px; }
    p { color: var(--accent); font-weight: 800; font-size: 12px; letter-spacing: .14em; }
    h1 { margin: 8px 0 14px; font-size: 44px; letter-spacing: -.04em; }
    span { color: var(--text-muted); line-height: 1.7; }
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HistoryPage {}
